import json, sys
GOLD = sys.argv[1]
d = json.load(open(GOLD))
base = d["memories"][:48]  # drop templated rows 48+ (repeats of rows 15-47 with new names)
NEW = [
 ("She moved to Lyon in 2022.", [], [], None, "2022-01-01"),
 ("I work at Kestrel Bio as a data engineer.", [("I","person"),("Kestrel Bio","organization")], [("I","works at","Kestrel Bio",False)], None, None),
 ("Team lead: Jonas Weber.", [("Jonas Weber","person")], [], None, None),
 ("Tomas left Brightwater in May 2024.", [("Tomas","person"),("Brightwater","organization")], [("Tomas","works at","Brightwater",True)], None, "2024-05-01"),
 ("Priya will join Nimbus Analytics in June 2026.", [("Priya","person"),("Nimbus Analytics","organization")], [("Priya","works at","Nimbus Analytics",False)], None, "2026-06-01"),
 ("Lena reviews the Orion roadmap every Friday.", [("Lena","person"),("Orion","project")], [("Lena","reviews","Orion",False)], None, None),
 ("Project Atlas has 14 engineers.", [("Project Atlas","project")], [], None, None),
 ("Node 20.11 is required for the build.", [("Node 20.11","software")], [], None, None),
 ("Lukas wohnt in Berlin.", [("Lukas","person"),("Berlin","place")], [("Lukas","lives in","Berlin",False)], None, None),
 ("Camille travaille chez Vertex Labs.", [("Camille","person"),("Vertex Labs","organization")], [("Camille","works at","Vertex Labs",False)], None, None),
 ("Hiroshi vive en Osaka.", [("Hiroshi","person"),("Osaka","place")], [("Hiroshi","lives in","Osaka",False)], None, None),
 ("Marta ya no trabaja en Sigma.", [("Marta","person"),("Sigma","organization")], [("Marta","works at","Sigma",True)], None, None),
 ("The deploy freeze starts on Friday.", [], [], None, None),
 ("Alex Kim joined Orbit Robotics in 2021.", [("Alex Kim","person"),("Orbit Robotics","organization")], [("Alex Kim","joined","Orbit Robotics",False)], None, "2021-01-01"),
 ("Alex Kim lives in Toronto.", [("Alex Kim","person"),("Toronto","place")], [("Alex Kim","lives in","Toronto",False)], None, None),
 ("Alex Ramirez lives in Madrid.", [("Alex Ramirez","person"),("Madrid","place")], [("Alex Ramirez","lives in","Madrid",False)], None, None),
 ("Owner: Dana Okafor owns the billing service.", [("Dana Okafor","person"),("billing service","software")], [("Dana Okafor","owns","billing service",False)], None, None),
 ("Sarah Chen joined Acme Corp as CTO and reports to Tom Lee.", [("Sarah Chen","person"),("Acme Corp","organization"),("Tom Lee","person")], [("Sarah Chen","joined","Acme Corp",False),("Sarah Chen","reports to","Tom Lee",False)], None, None),
 ("Tom Lee, who lives in Austin, manages the platform team.", [("Tom Lee","person"),("Austin","place"),("platform team","team")], [("Tom Lee","lives in","Austin",False),("Tom Lee","manages","platform team",False)], None, None),
 ("Ana prefers Python over Go for scripts.", [("Ana","person"),("Python","software"),("Go","software")], [("Ana","prefers","Python",False)], None, None),
 ("Ben doesn't like cilantro.", [("Ben","person"),("cilantro","food")], [("Ben","likes","cilantro",True)], None, None),
 ("Chloe does not work at Vance Capital anymore.", [("Chloe","person"),("Vance Capital","organization")], [("Chloe","works at","Vance Capital",True)], None, None),
 ("Diego reports to Priya Nair since 2025-02-03.", [("Diego","person"),("Priya Nair","person")], [("Diego","reports to","Priya Nair",False)], None, "2025-02-03"),
 ("Ellen moved to Denver in March 2023.", [("Ellen","person"),("Denver","place")], [("Ellen","moved to","Denver",False)], None, "2023-03-01"),
 ("Felix started at Quill Studio on 2024-09-16.", [("Felix","person"),("Quill Studio","organization")], [("Felix","works at","Quill Studio",False)], None, "2024-09-16"),
 ("Grace lives in Portland.", [("Grace","person"),("Portland","place")], [("Grace","lives in","Portland",False)], None, None),
 ("The payments service uses Redis 7.2.", [("payments service","software"),("Redis 7.2","software")], [("payments service","uses","Redis 7.2",False)], None, None),
 ("Our CI runs on GitHub Actions.", [("GitHub Actions","software")], [], None, None),
 ("Ravi maintains the parser repo.", [("Ravi","person"),("parser repo","project")], [("Ravi","maintains","parser repo",False)], None, None),
 ("Use the staging database on port 5433.", [], [], None, None),
 ("Mateo moved from Lima to Quito on 2025-06-01.", [("Mateo","person"),("Lima","place"),("Quito","place")], [("Mateo","moved to","Quito",False)], None, "2025-06-01"),
 ("Sakura joined Kyoto Bank in April 2020.", [("Sakura","person"),("Kyoto Bank","organization")], [("Sakura","joined","Kyoto Bank",False)], None, "2020-04-01"),
 ("Ruth prefers window seats on flights.", [("Ruth","person"),("window seats","preference")], [("Ruth","prefers","window seats",False)], None, None),
 ("Gareth no longer lives in Leeds; he now lives in York.", [("Gareth","person"),("Leeds","place"),("York","place")], [("Gareth","lives in","Leeds",True),("Gareth","lives in","York",False)], None, None),
 ("Our Q3 OKR is to cut latency by 20%.", [], [], None, None),
 ("Nikolai works for Helix Energy since 2019-09-01.", [("Nikolai","person"),("Helix Energy","organization")], [("Nikolai","works for","Helix Energy",False)], None, "2019-09-01"),
 ("Ingrid owns a Tesla Model 3.", [("Ingrid","person"),("Tesla Model 3","product")], [("Ingrid","owns","Tesla Model 3",False)], None, None),
 ("Kenji is employed at Ito Motors.", [("Kenji","person"),("Ito Motors","organization")], [("Kenji","works at","Ito Motors",False)], None, None),
 ("Priyanka lives in Bengaluru on 2023-11-20.", [("Priyanka","person"),("Bengaluru","place")], [("Priyanka","lives in","Bengaluru",False)], None, "2023-11-20"),
 ("The Phoenix migration is planned for next spring.", [("Phoenix","project")], [], None, None),
 ("Oscar and Mina work at Lumen Health.", [("Oscar","person"),("Mina","person"),("Lumen Health","organization")], [("Oscar","works at","Lumen Health",False),("Mina","works at","Lumen Health",False)], None, None),
 ("In 2021 Victor moved to Zurich.", [("Victor","person"),("Zurich","place")], [("Victor","moved to","Zurich",False)], None, "2021-01-01"),
 ("Team standup is every Monday at 9am.", [], [], None, None),
 ("Rhea uses Vim, not Emacs.", [("Rhea","person"),("Vim","software"),("Emacs","software")], [("Rhea","uses","Vim",False)], None, None),
 ("Bruno lives in Porto since 2022-05-10.", [("Bruno","person"),("Porto","place")], [("Bruno","lives in","Porto",False)], None, "2022-05-10"),
 ("Lina works at Orca Biotech as of 2026-01-05.", [("Lina","person"),("Orca Biotech","organization")], [("Lina","works at","Orca Biotech",False)], None, "2026-01-05"),
 ("Mei Ling reports to Frank Ogunleye.", [("Mei Ling","person"),("Frank Ogunleye","person")], [("Mei Ling","reports to","Frank Ogunleye",False)], None, None),
 ("Frank Ogunleye manages Team Nova.", [("Frank Ogunleye","person"),("Team Nova","team")], [("Frank Ogunleye","manages","Team Nova",False)], None, None),
 ("Simone prefers async standups.", [("Simone","person"),("async standups","preference")], [("Simone","prefers","async standups",False)], None, None),
 ("Yusuf lives in Istanbul, and Yusuf works at Bloom Foods.", [("Yusuf","person"),("Istanbul","place"),("Bloom Foods","organization")], [("Yusuf","lives in","Istanbul",False),("Yusuf","works at","Bloom Foods",False)], None, None),
 ("Hector no longer manages Project Lighthouse.", [("Hector","person"),("Project Lighthouse","project")], [("Hector","manages","Project Lighthouse",True)], None, None),
 ("Elif joined Nordwind in 2018.", [("Elif","person"),("Nordwind","organization")], [("Elif","joined","Nordwind",False)], None, "2018-01-01"),
 ("Akira reports to Mei Ling.", [("Akira","person"),("Mei Ling","person")], [("Akira","reports to","Mei Ling",False)], None, None),
 ("Hannah was promoted to staff engineer in 2024.", [("Hannah","person")], [], None, "2024-01-01"),
 ("Ferdinand likes jazz.", [("Ferdinand","person"),("jazz","preference")], [("Ferdinand","likes","jazz",False)], None, None),
 ("Rita lives in Lisbon and works at Tidewater.", [("Rita","person"),("Lisbon","place"),("Tidewater","organization")], [("Rita","lives in","Lisbon",False),("Rita","works at","Tidewater",False)], None, None),
 ("John Smith lives in Boston.", [("John Smith","person"),("Boston","place")], [("John Smith","lives in","Boston",False)], None, None),
 ("Jane Smith lives in Chicago.", [("Jane Smith","person"),("Chicago","place")], [("Jane Smith","lives in","Chicago",False)], None, None),
 ("The user prefers Neovim over VS Code.", [("the user","person"),("Neovim","software"),("VS Code","software")], [("the user","prefers","Neovim",False)], None, None),
 ("I moved to Montreal in 2024.", [("I","person"),("Montreal","place")], [("I","moved to","Montreal",False)], None, "2024-01-01"),
 ("I no longer work at Kestrel Bio.", [("I","person"),("Kestrel Bio","organization")], [("I","works at","Kestrel Bio",True)], None, None),
 ("Dr. Okonkwo works at Mercy General.", [("Dr. Okonkwo","person"),("Mercy General","organization")], [("Dr. Okonkwo","works at","Mercy General",False)], None, None),
 ("Prof. Ilse Brandt teaches at TU Delft.", [("Prof. Ilse Brandt","person"),("TU Delft","organization")], [("Prof. Ilse Brandt","teaches at","TU Delft",False)], None, None),
 ("Team Helios uses Postgres 16 for billing.", [("Team Helios","team"),("Postgres 16","software")], [("Team Helios","uses","Postgres 16",False)], None, None),
 ("Iva lives in Sofia on 2021-07-07.", [("Iva","person"),("Sofia","place")], [("Iva","lives in","Sofia",False)], None, "2021-07-07"),
 ("Julian reports to Nadia since last March.", [("Julian","person"),("Nadia","person")], [("Julian","reports to","Nadia",False)], None, None),
 ("Omar left Solstice in 2023.", [("Omar","person"),("Solstice","organization")], [("Omar","works at","Solstice",True)], None, "2023-01-01"),
 ("Peter Lang moved from Hamburg to Munich.", [("Peter Lang","person"),("Hamburg","place"),("Munich","place")], [("Peter Lang","moved to","Munich",False)], None, None),
 ("Zeynep owns Kite Coffee.", [("Zeynep","person"),("Kite Coffee","organization")], [("Zeynep","owns","Kite Coffee",False)], None, None),
 ("Nadia manages Team Helios.", [("Nadia","person"),("Team Helios","team")], [("Nadia","manages","Team Helios",False)], None, None),
 ("Marcus lives in Dublin on 2026-03-03.", [("Marcus","person"),("Dublin","place")], [("Marcus","lives in","Dublin",False)], None, "2026-03-03"),
 ("Wei works for Lattice Robotics.", [("Wei","person"),("Lattice Robotics","organization")], [("Wei","works for","Lattice Robotics",False)], None, None),
 ("Our staging environment runs Kubernetes 1.30.", [("Kubernetes 1.30","software")], [], None, None),
 ("Python 3.12 is the default interpreter on the build farm.", [("Python 3.12","software"),("build farm","software")], [], None, None),
 ("Jorge está en Valencia desde 2024-08-01.", [("Jorge","person"),("Valencia","place")], [], None, "2024-08-01"),
 ("Eva vive en Sevilla.", [("Eva","person"),("Sevilla","place")], [("Eva","lives in","Sevilla",False)], None, None),
 ("Kofi never worked at Vantage.", [("Kofi","person"),("Vantage","organization")], [("Kofi","works at","Vantage",True)], None, None),
 ("Leonie joined Brightline on 2025-10-01 and reports to Felix Brandt.", [("Leonie","person"),("Brightline","organization"),("Felix Brandt","person")], [("Leonie","joined","Brightline",False),("Leonie","reports to","Felix Brandt",False)], None, "2025-10-01"),
 ("Noor manages the infra team.", [("Noor","person"),("infra team","team")], [("Noor","manages","infra team",False)], None, None),
 ("The billing service uses Stripe.", [("billing service","software"),("Stripe","software")], [("billing service","uses","Stripe",False)], None, None),
]
mem = list(base)
for content, ents, rels, _, vf in NEW:
    m = {"content": content,
         "entities": [{"name": n, "type": t} for n, t in ents],
         "relations": [{"source": s, "predicate": p, "target": t, "negated": ng} for s, p, t, ng in rels]}
    if vf: m["valid_from"] = vf
    mem.append(m)
# sanity: every label must occur in its content
for m in mem:
    for e in m["entities"]:
        assert e["name"] in m["content"] or e["name"].lower() in m["content"].lower(), (e["name"], m["content"])
    for r in m["relations"]:
        for x in (r["source"], r["target"]):
            assert x in m["content"] or x.lower() in m["content"].lower(), (x, m["content"])
d["memories"] = mem
json.dump(d, open(GOLD, "w"), ensure_ascii=False, indent=1)
print(len(mem))
