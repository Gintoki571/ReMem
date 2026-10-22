use crate::{
    db::{vec_blob, Db},
    embed::{Embedder, Mode},
    engine::{Config, EntityIn, EntityResolution, RelationResult, RememberInput, RememberOutput},
    error::Result,
    extract, kg, resolve,
    temporal::{self, Clock},
    text,
};
use rusqlite::params;
use std::collections::HashMap;

pub fn check_embedder(db: &Db, emb: &dyn Embedder) -> Result<()> {
    let found: Option<String> = db
        .sql()
        .query_row("SELECT value FROM meta WHERE key='embedder'", [], |r| {
            r.get(0)
        })
        .optional()?;
    if let Some(found) = found {
        if found != emb.name() {
            return Err(crate::err!("this database was built with embedder '{}' but '{}' is active; use the same embedder or another --db", found, emb.name()));
        }
    }
    Ok(())
}

pub fn remember(
    db: &Db,
    emb: &dyn Embedder,
    cfg: &Config,
    clock: Clock,
    inp: RememberInput,
) -> Result<RememberOutput> {
    check_embedder(db, emb)?;
    let content = inp.content.trim().to_string();
    if content.is_empty() {
        return Err(crate::err!("content must not be empty"));
    }
    let kind = inp.kind.unwrap_or_else(|| "note".into());
    if !crate::engine::KINDS.contains(&kind.as_str()) {
        return Err(crate::err!(
            "invalid kind '{}'; valid kinds: {}",
            kind,
            crate::engine::KINDS.join(", ")
        ));
    }
    let valid_from = inp
        .valid_from
        .as_deref()
        .map(|s| {
            temporal::parse_instant(s, &clock).ok_or_else(|| {
                crate::err!(
                    "invalid valid_from '{}'; use ISO date, unix seconds, or 'yesterday'",
                    s
                )
            })
        })
        .transpose()?
        .unwrap_or(clock.now);
    let valid_to = inp
        .valid_to
        .as_deref()
        .map(|s| {
            temporal::parse_instant(s, &clock).ok_or_else(|| {
                crate::err!(
                    "invalid valid_to '{}'; use ISO date, unix seconds, or 'tomorrow'",
                    s
                )
            })
        })
        .transpose()?;
    if valid_to.is_some_and(|v| v <= valid_from) {
        return Err(crate::err!("valid_to must be after valid_from"));
    }
    let importance = inp
        .importance
        .filter(|x| !x.is_nan())
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    let mut tags: Vec<String> = inp
        .tags
        .iter()
        .chain(inp.keywords.iter())
        .map(|x| text::fold(x))
        .filter(|x| !x.is_empty())
        .collect();
    tags.sort();
    tags.dedup();
    let tags = tags.join(" ");
    let mut entities = inp.entities.clone();
    if entities.is_empty() {
        entities = extract::heuristic_entities(&content)
            .into_iter()
            .map(|c| EntityIn {
                name: c.name,
                kind: None,
                description: None,
                aliases: Vec::new(),
                same_as: None,
            })
            .collect();
    }
    let cards: Vec<String> = entities
        .iter()
        .map(|e| {
            format!(
                "{} ({}): {}",
                e.name,
                e.kind.as_deref().unwrap_or(""),
                e.description.as_deref().unwrap_or("")
            )
        })
        .collect();
    let mut relation_texts = Vec::new();
    for r in &inp.relations {
        let fact = r
            .fact
            .clone()
            .unwrap_or_else(|| format!("{} {} {}", r.source, r.predicate, r.target));
        relation_texts.push(fact);
    }
    let mut embed_texts = vec![content.as_str()];
    embed_texts.extend(cards.iter().map(String::as_str));
    embed_texts.extend(relation_texts.iter().map(String::as_str));
    let vectors = emb.embed(&embed_texts, Mode::Document)?;
    let hash = text::content_hash(&kind, &content);
    let duplicate: Option<i64> = db
        .sql()
        .query_row(
            "SELECT id FROM memories WHERE content_hash=?1 AND expired_at IS NULL LIMIT 1",
            [&hash],
            |r| r.get(0),
        )
        .optional()?;
    let near = knn_memory(db, &vectors[0], cfg.near_dup)?;
    let supersedes: Vec<i64> = inp
        .supersedes
        .iter()
        .map(|s| {
            crate::engine::parse_id(s, 'm')
                .ok_or_else(|| crate::err!("invalid supersedes id '{s}'"))
        })
        .collect::<Result<_>>()?;
    for id in &supersedes {
        let exists: bool = db.sql().query_row(
            "SELECT EXISTS(SELECT 1 FROM memories WHERE id=?1 AND expired_at IS NULL)",
            [id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(crate::err!(
                "supersedes target m{} is unknown or already expired",
                id
            ));
        }
    }
    db.write(|db| {
        let now=clock.now;
        let duplicate_of = duplicate;
        db.sql().execute("INSERT OR IGNORE INTO meta(key,value) VALUES('embedder',?1)",[emb.name()])?;
        let id = if let Some(id) = duplicate {
            db.sql().execute("UPDATE memories SET access_count=access_count+1,last_access=?1 WHERE id=?2",params![now,id])?;
            id
        } else {
            db.sql().execute("INSERT INTO memories(content,kind,tags,importance,scope,source,session,created_at,valid_from,valid_to,content_hash) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![content,kind,tags,importance,inp.scope.unwrap_or_default(),inp.source.unwrap_or_default(),inp.session.unwrap_or_default(),now,valid_from,valid_to,hash])?;
            let id=db.sql().last_insert_rowid();
            db.sql().execute("INSERT INTO memories_vec(rowid,embedding,live) VALUES (?1,?2,1)",params![id,vec_blob(&vectors[0])])?;
            kg::project_memory(db,id,&kind)?;
            id
        };
        let mut resolutions=Vec::new(); let mut ids=HashMap::new();
        for (i,e) in entities.iter().enumerate() {
            let r=resolve::resolve_entity(db,e,&vectors[i+1],now,cfg)?;
            let eid=r.id; ids.insert(text::norm_name(&e.name),eid);
            db.sql().execute("INSERT OR IGNORE INTO mentions(memory_id,entity_id) VALUES (?1,?2)",params![id,eid])?;
            db.sql().execute("UPDATE entities SET mention_count=mention_count+1 WHERE id=?1",[eid])?;
            kg::project_entity(db,eid,&e.name,e.kind.as_deref().unwrap_or(""))?;
            kg::project_mention(db,id,eid)?;
            for (other,_name,score) in &r.similar { let (a,b)=if eid<*other{(eid,*other)}else{(*other,eid)}; if a!=b {db.sql().execute("INSERT OR REPLACE INTO similar(a,b,score) VALUES (?1,?2,?3)",params![a,b,score])?;kg::project_similar(db,a,b,*score)?;}}
            resolutions.push(EntityResolution{name:e.name.clone(),id:eid,status:r.status,similar:r.similar});
        }
        let mut rel_results=Vec::new(); let mut invalidated=Vec::new();
        for (ri,r) in inp.relations.iter().enumerate() {
            let src=resolve_endpoint(db,&r.source,&ids,now,cfg,&vectors[1])?;
            let dst=resolve_endpoint(db,&r.target,&ids,now,cfg,&vectors[1])?;
            let (pid,pred)=resolve::resolve_predicate(db,&r.predicate,&vectors[1+entities.len()+ri],r.exclusive)?;
            let rf=r.valid_from.as_deref().map(|s|temporal::parse_instant(s,&clock).ok_or_else(||crate::err!("invalid relation valid_from '{s}'"))).transpose()?.unwrap_or(valid_from);
            let fact=r.fact.clone().unwrap_or_else(||format!("{} {} {}",r.source,r.predicate,r.target));
            if r.exclusive {
                let mut st=db.sql().prepare("SELECT id,valid_from FROM relations WHERE src=?1 AND predicate_id=?2 AND dst!=?3 AND expired_at IS NULL AND (valid_to IS NULL OR valid_to>?4)")?;
                let olds:Vec<(i64,i64)>=st.query_map(params![src,pid,dst,rf],|x|Ok((x.get(0)?,x.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
                for (old,of) in olds { if of<rf {db.sql().execute("UPDATE relations SET valid_to=?1 WHERE id=?2",[rf,old])?;} else {db.sql().execute("UPDATE relations SET valid_to=?1,expired_at=?2 WHERE id=?3",[rf,now,old])?;} invalidated.push(old);}
            }
            let existing:Option<(i64,i64)>=db.sql().query_row("SELECT id,evidence_count FROM relations WHERE src=?1 AND predicate_id=?2 AND dst=?3 AND expired_at IS NULL",[src,pid,dst],|x|Ok((x.get(0)?,x.get(1)?))).optional()?;
            let rid=if let Some((rid,count))=existing {db.sql().execute("UPDATE relations SET evidence_count=?1,weight=?1,valid_from=MIN(valid_from,?2) WHERE id=?3",params![count+1,rf,rid])?;db.sql().execute("INSERT OR IGNORE INTO relation_evidence VALUES(?1,?2)",params![rid,id])?;rid} else {db.sql().execute("INSERT INTO relations(src,dst,predicate_id,fact,weight,evidence_count,valid_from,valid_to,created_at) VALUES (?1,?2,?3,?4,1,1,?5,?6,?7)",params![src,dst,pid,fact,rf,r.valid_to.as_deref().and_then(|x|temporal::parse_instant(x,&clock)),now])?;let rid=db.sql().last_insert_rowid();db.sql().execute("INSERT INTO relation_evidence VALUES(?1,?2)",params![rid,id])?;db.sql().execute("INSERT INTO relations_fts(rowid,fact,predicate) VALUES (?1,?2,?3)",params![rid,r.fact.clone().unwrap_or_default(),pred])?;db.sql().execute("INSERT INTO relations_vec(rowid,embedding) VALUES(?1,?2)",params![rid,vec_blob(&vectors[1+entities.len()+ri])])?;rid};
            kg::project_relation(db,rid,src,dst,&pred)?; rel_results.push(RelationResult{id:rid,fact,status:if existing.is_some(){"merged".into()}else{"new".into()}});
        }
        for target in &supersedes {db.sql().execute("UPDATE memories SET valid_to=COALESCE(valid_to,?1),expired_at=?2,superseded_by=?3 WHERE id=?4",params![valid_from,now,id,target])?;db.sql().execute("UPDATE memories_vec SET live=0 WHERE rowid=?1",[target])?;}
        db.sql().execute("INSERT OR REPLACE INTO meta(key,value) VALUES('gen',COALESCE((SELECT CAST(value AS INTEGER)+1 FROM meta WHERE key='gen'),1))",[])?;
        Ok(RememberOutput{id,duplicate_of,near_duplicate:near,entities:resolutions,relations:rel_results,invalidated_relations:invalidated,superseded_memories:supersedes,hints:if content.len()>2000{vec!["long content: consider splitting into atomic memories".into()]}else{Vec::new()}})
    })
}

fn resolve_endpoint(
    db: &Db,
    name: &str,
    ids: &HashMap<String, i64>,
    now: i64,
    cfg: &Config,
    embedding: &[f32],
) -> Result<i64> {
    if let Some(id) = ids.get(&text::norm_name(name)) {
        return Ok(*id);
    };
    Ok(resolve::resolve_entity(
        db,
        &EntityIn {
            name: name.into(),
            ..Default::default()
        },
        embedding,
        now,
        cfg,
    )?
    .id)
}
fn knn_memory(db: &Db, v: &[f32], threshold: f32) -> Result<Option<(i64, f32)>> {
    let row:Option<(i64,f32)>=db.sql().query_row("SELECT rowid,1-distance FROM memories_vec WHERE embedding MATCH ?1 AND k=1 AND live=1 ORDER BY distance", [vec_blob(v)], |r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    Ok(row.filter(|(_, s)| *s >= threshold))
}
trait OptionalRow<T> {
    fn optional(self) -> rusqlite::Result<Option<T>>;
}
impl<T> OptionalRow<T> for rusqlite::Result<T> {
    fn optional(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
