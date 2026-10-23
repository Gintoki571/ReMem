//! Deterministic synthetic year-of-agent-memory generator.
//!
//! ```text
//! cargo run --release --example gen_world -- --people 300 --seed 7 --out eval/world-300.json
//! ```
//!
//! Produces an eval fixture (`now`, `memories`, `queries`) in the format
//! `neocortex eval` reads: a year of memories about `--people` people with
//! changing residences, jobs, managers, projects and tools, stable family,
//! pets and vehicles, and events, plus ground-truth queries whose `answers`
//! are the strings a correct top-10 context must contain.

use neocortex::export::fmt_iso;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

const DAY: i64 = 86_400;
/// 2026-10-21T15:00:00Z, the same anchor as eval/fixtures.json.
const NOW: i64 = 1_792_594_800;

// ------------------------------------------------------------------ randomness

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn between(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            lo
        } else {
            lo + (self.next() % (hi - lo) as u64) as i64
        }
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

// ------------------------------------------------------------------ name pools

const FIRST: &[(&str, bool)] = &[
    ("Aadhya", true),
    ("Aarohi", true),
    ("Abigail", true),
    ("Ada", true),
    ("Aditi", true),
    ("Aisha", true),
    ("Alice", true),
    ("Alma", true),
    ("Amara", true),
    ("Amelia", true),
    ("Amina", true),
    ("Ana", true),
    ("Ananya", true),
    ("Angela", true),
    ("Anika", true),
    ("Anna", true),
    ("Anouk", true),
    ("Aparna", true),
    ("Aria", true),
    ("Aroha", true),
    ("Asha", true),
    ("Aurora", true),
    ("Ava", true),
    ("Avani", true),
    ("Beatrice", true),
    ("Bella", true),
    ("Bianca", true),
    ("Camila", true),
    ("Carla", true),
    ("Carmen", true),
    ("Celia", true),
    ("Charlotte", true),
    ("Chloe", true),
    ("Clara", true),
    ("Cleo", true),
    ("Cora", true),
    ("Daisy", true),
    ("Dalia", true),
    ("Dana", true),
    ("Daniela", true),
    ("Daria", true),
    ("Deepa", true),
    ("Delia", true),
    ("Diana", true),
    ("Divya", true),
    ("Dora", true),
    ("Eesha", true),
    ("Elena", true),
    ("Elif", true),
    ("Elise", true),
    ("Ella", true),
    ("Elsa", true),
    ("Emma", true),
    ("Erika", true),
    ("Esther", true),
    ("Eva", true),
    ("Fatima", true),
    ("Fiona", true),
    ("Freya", true),
    ("Gabriela", true),
    ("Gauri", true),
    ("Gemma", true),
    ("Gia", true),
    ("Grace", true),
    ("Greta", true),
    ("Hana", true),
    ("Hannah", true),
    ("Harlow", true),
    ("Hazel", true),
    ("Helena", true),
    ("Hema", true),
    ("Ida", true),
    ("Imogen", true),
    ("Ines", true),
    ("Ira", true),
    ("Irene", true),
    ("Iris", true),
    ("Isabel", true),
    ("Isha", true),
    ("Iva", true),
    ("Ivy", true),
    ("Jaya", true),
    ("Jasmine", true),
    ("Jenna", true),
    ("Jia", true),
    ("Jiya", true),
    ("Joanna", true),
    ("Juhi", true),
    ("Julia", true),
    ("June", true),
    ("Kaia", true),
    ("Kavya", true),
    ("Keira", true),
    ("Kiran", true),
    ("Klara", true),
    ("Kriti", true),
    ("Laila", true),
    ("Lara", true),
    ("Laura", true),
    ("Layla", true),
    ("Leah", true),
    ("Leela", true),
    ("Lena", true),
    ("Leona", true),
    ("Lila", true),
    ("Lily", true),
    ("Lina", true),
    ("Lisa", true),
    ("Lucia", true),
    ("Luna", true),
    ("Lydia", true),
    ("Mabel", true),
    ("Maeve", true),
    ("Maja", true),
    ("Malini", true),
    ("Mara", true),
    ("Maria", true),
    ("Maya", true),
    ("Meera", true),
    ("Mia", true),
    ("Mira", true),
    ("Nadia", true),
    ("Naina", true),
    ("Naomi", true),
    ("Neha", true),
    ("Nia", true),
    ("Nina", true),
    ("Nora", true),
    ("Nyla", true),
    ("Olga", true),
    ("Olivia", true),
    ("Oona", true),
    ("Paige", true),
    ("Paloma", true),
    ("Pari", true),
    ("Paula", true),
    ("Priya", true),
    ("Rani", true),
    ("Rhea", true),
    ("Rosa", true),
    ("Ruby", true),
    ("Ruhi", true),
    ("Saanvi", true),
    ("Sadie", true),
    ("Salma", true),
    ("Sara", true),
    ("Selma", true),
    ("Shreya", true),
    ("Sia", true),
    ("Sima", true),
    ("Sofia", true),
    ("Sonal", true),
    ("Stela", true),
    ("Suki", true),
    ("Tanya", true),
    ("Tara", true),
    ("Tessa", true),
    ("Thea", true),
    ("Trisha", true),
    ("Uma", true),
    ("Valentina", true),
    ("Vera", true),
    ("Vidya", true),
    ("Vina", true),
    ("Wren", true),
    ("Yara", true),
    ("Yasmin", true),
    ("Yuki", true),
    ("Zara", true),
    ("Zoe", true),
    ("Aarav", false),
    ("Aaron", false),
    ("Adam", false),
    ("Adil", false),
    ("Adrian", false),
    ("Ahmed", false),
    ("Aiden", false),
    ("Ajay", false),
    ("Alan", false),
    ("Alberto", false),
    ("Alex", false),
    ("Ali", false),
    ("Aman", false),
    ("Amir", false),
    ("Anand", false),
    ("Anders", false),
    ("Andre", false),
    ("Andrew", false),
    ("Anil", false),
    ("Anton", false),
    ("Arjun", false),
    ("Arman", false),
    ("Arun", false),
    ("Ash", false),
    ("Ashwin", false),
    ("Atul", false),
    ("Avinash", false),
    ("Ben", false),
    ("Bilal", false),
    ("Blake", false),
    ("Boris", false),
    ("Bruno", false),
    ("Caleb", false),
    ("Carlos", false),
    ("Cedric", false),
    ("Charlie", false),
    ("Chris", false),
    ("Cole", false),
    ("Colin", false),
    ("Daniel", false),
    ("Danny", false),
    ("David", false),
    ("Deepak", false),
    ("Dev", false),
    ("Diego", false),
    ("Dmitri", false),
    ("Dominic", false),
    ("Dylan", false),
    ("Eamon", false),
    ("Edgar", false),
    ("Eduardo", false),
    ("Eli", false),
    ("Elias", false),
    ("Elliot", false),
    ("Emil", false),
    ("Emre", false),
    ("Eric", false),
    ("Ethan", false),
    ("Evan", false),
    ("Ezra", false),
    ("Farid", false),
    ("Felix", false),
    ("Finn", false),
    ("Frank", false),
    ("Gabriel", false),
    ("Gavin", false),
    ("George", false),
    ("Gopal", false),
    ("Gordon", false),
    ("Graham", false),
    ("Guy", false),
    ("Harish", false),
    ("Harry", false),
    ("Hassan", false),
    ("Hector", false),
    ("Henry", false),
    ("Hugo", false),
    ("Ian", false),
    ("Ibrahim", false),
    ("Idris", false),
    ("Igor", false),
    ("Imran", false),
    ("Isaac", false),
    ("Ishan", false),
    ("Ivan", false),
    ("Jack", false),
    ("Jacob", false),
    ("Jai", false),
    ("James", false),
    ("Jason", false),
    ("Javier", false),
    ("Jay", false),
    ("Jeevan", false),
    ("Jerome", false),
    ("Joel", false),
    ("Johan", false),
    ("John", false),
    ("Jonas", false),
    ("Jose", false),
    ("Joseph", false),
    ("Josh", false),
    ("Julian", false),
    ("Kabir", false),
    ("Karan", false),
    ("Karthik", false),
    ("Ken", false),
    ("Kevin", false),
    ("Kian", false),
    ("Kishore", false),
    ("Krish", false),
    ("Kunal", false),
    ("Kurt", false),
    ("Leo", false),
    ("Leon", false),
    ("Liam", false),
    ("Logan", false),
    ("Lucas", false),
    ("Luis", false),
    ("Lukas", false),
    ("Manav", false),
    ("Marco", false),
    ("Marcus", false),
    ("Mario", false),
    ("Mark", false),
    ("Martin", false),
    ("Mateo", false),
    ("Max", false),
    ("Mehul", false),
    ("Miguel", false),
    ("Mihir", false),
    ("Mike", false),
    ("Milan", false),
    ("Mohan", false),
    ("Nadav", false),
    ("Nakul", false),
    ("Nathan", false),
    ("Neal", false),
    ("Neel", false),
    ("Neil", false),
    ("Nick", false),
    ("Nikhil", false),
    ("Noah", false),
    ("Noel", false),
    ("Oliver", false),
    ("Omar", false),
    ("Oscar", false),
    ("Owen", false),
    ("Pablo", false),
    ("Paolo", false),
    ("Parth", false),
    ("Patrick", false),
    ("Paul", false),
    ("Pedro", false),
    ("Peter", false),
    ("Pranav", false),
    ("Prashant", false),
    ("Qasim", false),
    ("Rahul", false),
    ("Raj", false),
    ("Rajesh", false),
    ("Rakesh", false),
    ("Rami", false),
    ("Ranjit", false),
    ("Ravi", false),
    ("Ray", false),
    ("Rehan", false),
    ("Rishi", false),
    ("Rob", false),
    ("Rohan", false),
    ("Roman", false),
    ("Rory", false),
    ("Ruben", false),
    ("Ryan", false),
    ("Sahil", false),
    ("Salim", false),
    ("Sam", false),
    ("Samir", false),
    ("Sanjay", false),
    ("Sasha", false),
    ("Scott", false),
    ("Sean", false),
    ("Sebastian", false),
    ("Sergio", false),
    ("Shane", false),
    ("Shiv", false),
    ("Siddharth", false),
    ("Simon", false),
    ("Soren", false),
    ("Stefan", false),
    ("Stephen", false),
    ("Suresh", false),
    ("Tanmay", false),
    ("Tariq", false),
    ("Ted", false),
    ("Theo", false),
    ("Thomas", false),
    ("Tim", false),
    ("Tobias", false),
    ("Tom", false),
    ("Umar", false),
    ("Varun", false),
    ("Ved", false),
    ("Victor", false),
    ("Vijay", false),
    ("Vikram", false),
    ("Vinay", false),
    ("Vishal", false),
    ("Vivek", false),
    ("Wesley", false),
    ("Will", false),
    ("Xavier", false),
    ("Yash", false),
    ("Yusuf", false),
    ("Zach", false),
    ("Zane", false),
];

const SURNAME_PREFIX: &[&str] = &[
    "Agar", "Ahlu", "Bak", "Ban", "Bar", "Bhat", "Bis", "Cha", "Chan", "Chau", "Dal", "Dar",
    "Davi", "Desh", "Dhar", "Dix", "Dube", "Gan", "Garg", "Gho", "Gup", "Har", "Jai", "Jha", "Kap",
    "Kha", "Koh", "Kri", "Kul", "Lal", "Mad", "Mah", "Mal", "Man", "Meh", "Mis", "Moh", "Nai",
    "Nar", "Obe", "Pad", "Pan", "Par", "Pat", "Pil", "Pra", "Rag", "Raj", "Ram", "Rao", "Red",
    "Roy", "Sah", "Sai", "Sam", "San", "Sar", "Sen", "Sha", "She", "Shi", "Sin", "Sod", "Sri",
    "Tam", "Tan", "Tha", "Tiw", "Tri", "Upa", "Vai", "Var", "Ver", "Vij", "Voh", "Wag", "Yad",
    "Zai",
];

const SURNAME_SUFFIX: &[&str] = &[
    "ani", "appa", "bhai", "chand", "dhar", "dran", "gan", "gari", "kar", "kumar", "lal", "mal",
    "mani", "nath", "pati", "pore", "prasad", "raj", "ram", "rani", "rao", "sai", "sen", "shah",
    "shen", "singh", "sri", "vari", "vati", "wala", "wani", "yya", "zai", "zada", "zadeh",
];

const CITIES: &[&str] = &[
    "Pune",
    "Delhi",
    "Mumbai",
    "Bengaluru",
    "Chennai",
    "Hyderabad",
    "Kolkata",
    "Ahmedabad",
    "Jaipur",
    "Surat",
    "Lucknow",
    "Kanpur",
    "Nagpur",
    "Indore",
    "Bhopal",
    "Patna",
    "Vadodara",
    "Coimbatore",
    "Kochi",
    "Thiruvananthapuram",
    "Guwahati",
    "Chandigarh",
    "Dehradun",
    "Shimla",
    "Ranchi",
    "Raipur",
    "Bhubaneswar",
    "Visakhapatnam",
    "Mysuru",
    "Mangaluru",
    "Nashik",
    "Aurangabad",
    "Jodhpur",
    "Udaipur",
    "Agra",
    "Varanasi",
    "Amritsar",
    "Ludhiana",
    "Srinagar",
    "Leh",
    "Karachi",
    "Lahore",
    "Islamabad",
    "Dhaka",
    "Kathmandu",
    "Colombo",
    "Male",
    "Dubai",
    "Abu Dhabi",
    "Doha",
    "Riyadh",
    "Jeddah",
    "Muscat",
    "Kuwait City",
    "Manama",
    "Tehran",
    "Istanbul",
    "Ankara",
    "Tel Aviv",
    "Cairo",
    "Alexandria",
    "Casablanca",
    "Tunis",
    "Algiers",
    "Nairobi",
    "Mombasa",
    "Addis Ababa",
    "Lagos",
    "Accra",
    "Dakar",
    "Johannesburg",
    "Cape Town",
    "Durban",
    "Kigali",
    "Kampala",
    "Dar es Salaam",
    "London",
    "Manchester",
    "Birmingham",
    "Edinburgh",
    "Glasgow",
    "Dublin",
    "Belfast",
    "Cardiff",
    "Paris",
    "Lyon",
    "Marseille",
    "Toulouse",
    "Nice",
    "Bordeaux",
    "Madrid",
    "Barcelona",
    "Valencia",
    "Seville",
    "Lisbon",
    "Porto",
    "Rome",
    "Milan",
    "Naples",
    "Turin",
    "Florence",
    "Venice",
    "Berlin",
    "Munich",
    "Hamburg",
    "Cologne",
    "Frankfurt",
    "Stuttgart",
    "Dusseldorf",
    "Vienna",
    "Salzburg",
    "Zurich",
    "Geneva",
    "Basel",
    "Bern",
    "Amsterdam",
    "Rotterdam",
    "Brussels",
    "Antwerp",
    "Copenhagen",
    "Oslo",
    "Stockholm",
    "Helsinki",
    "Reykjavik",
    "Warsaw",
    "Krakow",
    "Prague",
    "Budapest",
    "Bucharest",
    "Sofia",
    "Belgrade",
    "Zagreb",
    "Ljubljana",
    "Sarajevo",
    "Skopje",
    "Tirana",
    "Athens",
    "Thessaloniki",
    "Kyiv",
    "Lviv",
    "Moscow",
    "Saint Petersburg",
    "Minsk",
    "Riga",
    "Vilnius",
    "Tallinn",
    "New York City",
    "Los Angeles",
    "Chicago",
    "San Francisco",
    "Seattle",
    "Boston",
    "Austin",
    "Denver",
    "Miami",
    "Atlanta",
    "Dallas",
    "Houston",
    "Phoenix",
    "Portland",
    "San Diego",
    "Philadelphia",
    "Washington",
    "Toronto",
    "Vancouver",
    "Montreal",
    "Ottawa",
    "Calgary",
    "Mexico City",
    "Guadalajara",
    "Monterrey",
    "Bogota",
    "Medellin",
    "Lima",
    "Quito",
    "Santiago",
    "Buenos Aires",
    "Montevideo",
    "Sao Paulo",
    "Rio de Janeiro",
    "Brasilia",
    "Salvador",
    "Recife",
    "Caracas",
    "Panama City",
    "Tokyo",
    "Osaka",
    "Kyoto",
    "Nagoya",
    "Fukuoka",
    "Sapporo",
    "Seoul",
    "Busan",
    "Beijing",
    "Shanghai",
    "Shenzhen",
    "Guangzhou",
    "Hangzhou",
    "Chengdu",
    "Wuhan",
    "Hong Kong",
    "Taipei",
    "Kaohsiung",
    "Singapore",
    "Kuala Lumpur",
    "Penang",
    "Jakarta",
    "Surabaya",
    "Manila",
    "Cebu",
    "Bangkok",
    "Chiang Mai",
    "Hanoi",
    "Ho Chi Minh City",
    "Da Nang",
    "Phnom Penh",
    "Yangon",
    "Sydney",
    "Melbourne",
    "Brisbane",
    "Perth",
    "Adelaide",
    "Auckland",
    "Wellington",
    "Christchurch",
    "Honolulu",
];

const TOOLS: &[&str] = &[
    "Rust",
    "Go",
    "Python",
    "TypeScript",
    "Java",
    "Kotlin",
    "Swift",
    "Scala",
    "Elixir",
    "Haskell",
    "C++",
    "C#",
    "Ruby",
    "PHP",
    "Zig",
    "Julia",
    "R",
    "Dart",
    "Lua",
    "Clojure",
    "PostgreSQL",
    "SQLite",
    "Redis",
    "Kafka",
    "Docker",
    "Kubernetes",
    "Terraform",
    "GraphQL",
    "gRPC",
    "PyTorch",
];

const LANGUAGES: &[&str] = &[
    "Rust",
    "Go",
    "Python",
    "TypeScript",
    "Java",
    "Kotlin",
    "Swift",
    "Scala",
    "Elixir",
    "C++",
    "C#",
    "Ruby",
    "Julia",
    "R",
    "Dart",
    "Zig",
];

const PET_NAMES: &[&str] = &[
    "Luna", "Milo", "Bella", "Max", "Charlie", "Coco", "Daisy", "Rocky", "Simba", "Nala", "Oscar",
    "Leo", "Zoe", "Lucy", "Toby", "Ruby", "Pepper", "Ginger", "Shadow", "Sunny", "Buddy", "Teddy",
    "Ollie", "Jasper", "Finn", "Cleo", "Mango", "Biscuit", "Peanut", "Hazel", "Willow", "Maple",
    "Juniper", "Basil", "Clover", "Nutmeg", "Pickle", "Pumpkin", "Mochi", "Socks",
];

const PET_SPECIES: &[&str] = &["dog", "cat", "parrot", "rabbit", "turtle"];

const VEHICLES: &[&str] = &[
    "Honda Civic",
    "Toyota Corolla",
    "Maruti Swift",
    "Hyundai i20",
    "Tata Nexon",
    "Mahindra XUV",
    "Ford Focus",
    "Volkswagen Golf",
    "Skoda Octavia",
    "Renault Duster",
    "Kia Seltos",
    "Nissan Leaf",
    "Tesla Model 3",
    "BMW 3 Series",
    "Audi A4",
    "Mercedes C-Class",
    "Volvo XC40",
    "Mazda 3",
    "Subaru Outback",
    "Jeep Wrangler",
    "Toyota Fortuner",
    "Honda City",
    "Hyundai Creta",
    "Suzuki Baleno",
    "MG Hector",
    "Peugeot 208",
    "Fiat Panda",
    "Dacia Duster",
    "Mini Cooper",
    "Porsche 911",
];

const CONFERENCES: &[&str] = &[
    "PyCon",
    "RustConf",
    "KubeCon",
    "NeurIPS",
    "ICML",
    "ICLR",
    "WWW",
    "SIGMOD",
    "VLDB",
    "OSDI",
    "SOSP",
    "Strata",
    "QCon",
    "Devoxx",
    "GopherCon",
    "JSConf",
    "ReactConf",
    "DockerCon",
    "AWS re:Invent",
    "Google I/O",
    "Apple WWDC",
    "Microsoft Build",
    "Data Council",
    "ML Summit",
    "AI Summit",
    "TechCrunch Disrupt",
    "SXSW",
    "FOSDEM",
    "EuroPython",
    "Scala Days",
    "Open Source Summit",
    "CloudNativeCon",
];

const ORG_PREFIX: &[&str] = &[
    "Northwind",
    "Acme",
    "Globex",
    "Initech",
    "Umbrella",
    "Hooli",
    "Vandelay",
    "Stark",
    "Wayne",
    "Cyberdyne",
    "Tyrell",
    "Aperture",
    "Blackrock",
    "Bluewave",
    "Brightpath",
    "Cedar",
    "Cobalt",
    "Copperfield",
    "Delta",
    "Eastgate",
    "Everest",
    "Falcon",
    "Fathom",
    "Granite",
    "Harbor",
    "Helix",
    "Horizon",
    "Ironclad",
    "Juniper",
    "Keystone",
    "Lumen",
    "Meridian",
    "Nimbus",
    "Northstar",
    "Oakline",
    "Orbit",
    "Pinnacle",
    "Quantum",
    "Redwood",
    "Riverstone",
    "Sable",
    "Sandstone",
    "Silverline",
    "Skyward",
    "Solstice",
    "Summit",
    "Terra",
    "Thornhill",
    "Tidewater",
    "Trailhead",
    "Vantage",
    "Vertex",
    "Vistula",
    "Westbrook",
    "Willow",
    "Windward",
    "Zephyr",
    "Zenith",
    "Atlas",
    "Beacon",
    "Cascade",
];

const ORG_SUFFIX: &[&str] = &[
    "Labs",
    "Systems",
    "Analytics",
    "Robotics",
    "Health",
    "Capital",
    "Logistics",
    "Media",
    "Foods",
    "Energy",
    "Networks",
    "Studio",
    "Bank",
    "Motors",
    "Pharma",
    "Software",
    "Digital",
    "Partners",
    "Group",
    "Works",
];

const PROJECT_CODENAME: &[&str] = &[
    "Atlas", "Orion", "Vega", "Lyra", "Nova", "Comet", "Falcon", "Phoenix", "Titan", "Aurora",
    "Borealis", "Cobalt", "Delta", "Echo", "Ember", "Everest", "Gemini", "Helios", "Horizon",
    "Indigo", "Iris", "Juno", "Kestrel", "Lambda", "Lumen", "Meridian", "Nebula", "Nimbus", "Onyx",
    "Opal", "Orbit", "Pulsar", "Quasar", "Quartz", "Raven", "Rigel", "Sable", "Sirius", "Solstice",
    "Summit", "Terra", "Thule", "Umbra", "Vela", "Vertex", "Vesper", "Willow", "Xenon", "Yonder",
    "Zephyr", "Zenith", "Aegis", "Boreal", "Cypher", "Drift", "Flux", "Glide", "Halo", "Ion",
    "Kite",
];

const PROJECT_NOUN: &[&str] = &[
    "Platform",
    "API",
    "Pipeline",
    "Console",
    "Engine",
    "Gateway",
    "Service",
    "Toolkit",
    "Portal",
    "Studio",
    "Runtime",
    "Compiler",
    "Dashboard",
    "Scheduler",
    "Ledger",
    "Registry",
    "Inbox",
    "Sync",
    "Index",
    "Cache",
    "Router",
    "Monitor",
    "Bridge",
    "Forge",
    "Relay",
    "Vault",
    "Stream",
    "Canvas",
    "Compass",
    "Anchor",
    "Prism",
    "Lantern",
    "Signal",
    "Lattice",
    "Matrix",
    "Kernel",
    "Harness",
    "Circuit",
    "Beacon",
    "Archive",
];

const MONTHS: &[&str] = &[
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

// ------------------------------------------------------------------ world model

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Attr {
    Home,
    Job,
    Manager,
    Tool,
    Lead,
    Family,
    Pet,
    Vehicle,
    Conference,
    Trip,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum FactId {
    Person(usize, Attr, usize),
    Project(usize),
}

#[derive(Clone, Copy)]
struct Change {
    at: i64,
    value: usize,
}

struct Person {
    name: String,
    first: String,
    female: bool,
    home: Vec<Change>,
    job: Vec<Change>,
    manager: Vec<Change>,
    tool: Vec<Change>,
    /// Project this person currently leads, if any.
    lead: Option<Change>,
    /// The project-lead fact was written structured (the graph has the edge).
    lead_structured: bool,
    /// (relative, kind) — one family fact per person.
    family: Option<(usize, &'static str)>,
    /// The family fact was written structured (so the graph has the edge).
    family_structured: bool,
    pets: Vec<usize>,
    vehicle: Option<usize>,
    /// (when, conference, city)
    conference: Option<(i64, usize, usize)>,
    trip: Option<(i64, usize)>,
}

struct Mem {
    id: usize,
    at: i64,
    value: Value,
    fact: Option<FactId>,
}

fn date(ts: i64) -> String {
    fmt_iso(ts)[..10].to_string()
}

fn month_year(ts: i64) -> String {
    let text = fmt_iso(ts);
    let month: usize = text[5..7].parse().unwrap();
    format!("{} {}", MONTHS[month - 1], &text[..4])
}

fn ent(name: &str, kind: &str) -> Value {
    json!({"name": name, "type": kind})
}

fn rel(source: &str, predicate: &str, target: &str, exclusive: bool, fact: &str) -> Value {
    json!({"source": source, "predicate": predicate, "target": target,
           "fact": fact, "exclusive": exclusive})
}

#[allow(clippy::too_many_arguments)]
fn memory(
    content: String,
    kind: &str,
    tag: &str,
    importance: f64,
    at: i64,
    valid_to: Option<i64>,
    entities: Vec<Value>,
    relations: Vec<Value>,
    fact: Option<FactId>,
    id: usize,
) -> Mem {
    let mut value = json!({
        "content": content,
        "kind": kind,
        "tags": [tag],
        "importance": importance,
        "valid_from": date(at),
    });
    if let Some(to) = valid_to {
        value["valid_to"] = json!(date(to));
    }
    if !entities.is_empty() {
        value["entities"] = Value::Array(entities);
    }
    if !relations.is_empty() {
        value["relations"] = Value::Array(relations);
    }
    Mem {
        id,
        at,
        value,
        fact,
    }
}

/// A year of memory plus the facts needed to write queries about it.
struct World {
    people: Vec<Person>,
    orgs: Vec<String>,
    projects: Vec<String>,
    project_language: Vec<usize>,
    cities: Vec<String>,
    tools: Vec<String>,
    pets: Vec<String>,
    vehicles: Vec<String>,
    conferences: Vec<String>,
    /// Project index -> the person who leads it now.
    current_leads: Vec<usize>,
    mems: Vec<Mem>,
}

fn build(rng: &mut Rng, people_count: usize) -> World {
    let today = NOW.div_euclid(DAY);
    let start = today - 365;

    // People: unique full names; the first-name pool is shared, so first-name
    // collisions are common while full names stay distinct.
    let mut used_names: HashSet<String> = HashSet::new();
    let mut people = Vec::with_capacity(people_count);
    for _ in 0..people_count {
        let (first, female) = *rng.pick(FIRST);
        let mut name = String::new();
        for _ in 0..1000 {
            let surname = format!("{}{}", rng.pick(SURNAME_PREFIX), rng.pick(SURNAME_SUFFIX));
            name = format!("{first} {surname}");
            if !used_names.contains(&name) {
                break;
            }
        }
        used_names.insert(name.clone());
        people.push(Person {
            name,
            first: first.to_string(),
            female,
            home: Vec::new(),
            job: Vec::new(),
            manager: Vec::new(),
            tool: Vec::new(),
            lead: None,
            lead_structured: rng.chance(60),
            family: None,
            family_structured: false,
            pets: Vec::new(),
            vehicle: None,
            conference: None,
            trip: None,
        });
    }

    // Organisations, projects, cities.
    let org_count = (people_count / 20).max(2);
    let mut orgs: Vec<String> = Vec::new();
    while orgs.len() < org_count {
        let name = format!("{} {}", rng.pick(ORG_PREFIX), rng.pick(ORG_SUFFIX));
        if !orgs.contains(&name) {
            orgs.push(name);
        }
    }
    let project_count = (people_count / 10).max(2);
    let mut projects: Vec<String> = Vec::new();
    while projects.len() < project_count {
        let name = format!("{} {}", rng.pick(PROJECT_CODENAME), rng.pick(PROJECT_NOUN));
        if !projects.contains(&name) {
            projects.push(name);
        }
    }
    let project_language: Vec<usize> = (0..project_count)
        .map(|_| rng.below(LANGUAGES.len()))
        .collect();
    let cities: Vec<String> = CITIES.iter().map(|c| c.to_string()).collect();
    let tools: Vec<String> = TOOLS.iter().map(|t| t.to_string()).collect();
    let vehicles: Vec<String> = VEHICLES.iter().map(|v| v.to_string()).collect();
    let conferences: Vec<String> = CONFERENCES.iter().map(|c| c.to_string()).collect();

    let mut pets: Vec<String> = Vec::new();

    let ts = |rng: &mut Rng, day: i64| day * DAY + rng.between(9, 18) * 3600;

    // Timelines.
    for (index, person) in people.iter_mut().enumerate() {
        let home_start = start + rng.between(0, 25);
        let home_move = home_start + rng.between(100, 280);
        let home_move = home_move.min(today - 8);
        let city_a = rng.below(cities.len());
        let mut city_b = rng.below(cities.len());
        if city_b == city_a {
            city_b = (city_b + 1) % cities.len();
        }
        person.home = vec![
            Change {
                at: ts(rng, home_start),
                value: city_a,
            },
            Change {
                at: ts(rng, home_move),
                value: city_b,
            },
        ];

        let job_start = start + rng.between(0, 40);
        let job_move = job_start + rng.between(120, 300);
        let job_move = job_move.min(today - 5);
        let org_a = rng.below(orgs.len());
        let mut org_b = rng.below(orgs.len());
        if org_b == org_a {
            org_b = (org_b + 1) % orgs.len();
        }
        person.job = vec![
            Change {
                at: ts(rng, job_start),
                value: org_a,
            },
            Change {
                at: ts(rng, job_move),
                value: org_b,
            },
        ];

        let manager_day = start + rng.between(0, 60);
        let manager_at = ts(rng, manager_day);
        let mut manager = rng.below(people_count);
        if manager == index {
            manager = (manager + 1) % people_count;
        }
        person.manager = vec![Change {
            at: manager_at,
            value: manager,
        }];
        if rng.chance(40) {
            let later = (manager_at / DAY + rng.between(90, 250)).min(today - 3);
            let mut second = rng.below(people_count);
            while second == index || second == manager {
                second = rng.below(people_count);
            }
            person.manager.push(Change {
                at: ts(rng, later),
                value: second,
            });
        }

        let tool_day = start + rng.between(0, 30);
        let tool_at = ts(rng, tool_day);
        let tool = rng.below(tools.len());
        person.tool = vec![Change {
            at: tool_at,
            value: tool,
        }];
        if rng.chance(40) {
            let later = (tool_at / DAY + rng.between(80, 260)).min(today - 3);
            let mut second = rng.below(tools.len());
            while second == tool {
                second = (second + 1) % tools.len();
            }
            person.tool.push(Change {
                at: ts(rng, later),
                value: second,
            });
        }

        // One pet, two for a third of the people.
        let pet_count = if rng.chance(30) { 2 } else { 1 };
        for _ in 0..pet_count {
            // Pet names repeat across owners, as they do in life.
            let mut name = String::new();
            for _ in 0..8 {
                let species = *rng.pick(PET_SPECIES);
                let candidate = format!("{} the {}", rng.pick(PET_NAMES), species);
                if !person.pets.iter().any(|p| pets[*p] == candidate) {
                    name = candidate;
                    break;
                }
            }
            if name.is_empty() {
                name = format!("{} the dog", pets.len());
            }
            pets.push(name);
            let at = pets.len() - 1;
            person.pets.push(at);
        }

        if rng.chance(70) {
            person.vehicle = Some(rng.below(vehicles.len()));
        }

        // One person in five attended a conference in the previous calendar
        // month, so "last month" queries have an answer.
        let conference_day = if index % 5 == 0 {
            today - 49 + rng.between(0, 22)
        } else {
            start + rng.between(10, 350)
        };
        let conference_at = ts(rng, conference_day);
        let conference_city = rng.below(cities.len());
        person.conference = Some((conference_at, rng.below(conferences.len()), conference_city));

        if rng.chance(70) {
            let trip_day = start + rng.between(10, 350);
            let trip_at = ts(rng, trip_day);
            let trip_city = rng.below(cities.len());
            person.trip = Some((trip_at, trip_city));
        }
    }

    // Family: sibling pairs, parents and spouses. Only siblings are used for
    // 3-hop queries, and only their own family memory is structured for those.
    let mut taken = vec![false; people_count];
    for index in 0..people_count {
        if taken[index] {
            continue;
        }
        let roll = rng.below(3);
        let relative = match roll {
            0 if index + 1 < people_count && !taken[index + 1] => {
                taken[index + 1] = true;
                Some((index + 1, "sibling"))
            }
            1 if index > 0 => {
                let parent = rng.below(index);
                if taken[parent] {
                    None
                } else {
                    taken[parent] = true;
                    Some((parent, "parent"))
                }
            }
            _ if index + 1 < people_count && !taken[index + 1] => {
                taken[index + 1] = true;
                Some((index + 1, "spouse"))
            }
            _ => None,
        };
        if let Some((relative, kind)) = relative {
            taken[index] = true;
            people[index].family = Some((relative, kind));
            people[index].family_structured = kind == "sibling" && rng.chance(40);
        }
    }

    // Exactly one lead per project at a time; a third of the projects change
    // hands mid-year. A project with ten concurrent leads would be a graph hub
    // no traversal could rank through, and real teams do not work that way.
    let block = (people_count / projects.len().max(1)).max(2);
    let mut current_leads = vec![0usize; projects.len()];
    for (project, slot) in current_leads.iter_mut().enumerate() {
        let low = (project * block).min(people_count - 1);
        let high = (low + block).min(people_count);
        let lead = low + rng.below(high - low);
        let lead_day = start + rng.between(0, 45);
        let at = ts(rng, lead_day);
        people[lead].lead = Some(Change { at, value: project });
        *slot = lead;
        if rng.chance(30) && high - low > 1 {
            let mut next = low + rng.below(high - low);
            while next == lead {
                next = low + rng.below(high - low);
            }
            let next_day = start + rng.between(120, 320);
            let at = ts(rng, next_day);
            people[next].lead = Some(Change { at, value: project });
            *slot = next;
        }
    }

    let mut world = World {
        people,
        orgs,
        projects,
        project_language,
        cities,
        tools,
        pets,
        vehicles,
        conferences,
        current_leads,
        mems: Vec::new(),
    };
    emit_memories(&mut world, rng, start, today);
    world
}

#[allow(clippy::too_many_arguments)]
fn push_mem(
    world: &mut World,
    rng: &mut Rng,
    content: String,
    kind: &str,
    tag: &str,
    at: i64,
    valid_to: Option<i64>,
    entities: Vec<Value>,
    relations: Vec<Value>,
    fact: Option<FactId>,
) {
    let importance = 0.40 + rng.below(41) as f64 / 100.0;
    let id = world.mems.len();
    world.mems.push(memory(
        content, kind, tag, importance, at, valid_to, entities, relations, fact, id,
    ));
}

fn emit_memories(world: &mut World, rng: &mut Rng, start: i64, today: i64) {
    // Projects state their language once, at the start of the year.
    let project_languages: Vec<(String, String)> = world
        .projects
        .iter()
        .zip(world.project_language.iter())
        .map(|(project, language)| (project.clone(), world.tools[*language].clone()))
        .collect();
    for (project, (name, language)) in project_languages.into_iter().enumerate() {
        let content = match rng.below(3) {
            0 => format!("The {name} project uses {language}."),
            1 => format!("{name} is written in {language}."),
            _ => format!("The main language of {name} is {language}."),
        };
        let project_at = (start + rng.between(0, 10)) * DAY + 10 * 3600;
        push_mem(
            world,
            rng,
            content,
            "fact",
            "project",
            project_at,
            None,
            vec![ent(&name, "project"), ent(&language, "tool")],
            vec![rel(
                &name,
                "uses",
                &language,
                false,
                &format!("{name} uses {language}"),
            )],
            Some(FactId::Project(project)),
        );
    }

    let people_count = world.people.len();
    for index in 0..people_count {
        let person = &world.people[index];
        let name = person.name.clone();
        let first = person.first.clone();
        let home = person.home.clone();
        let job = person.job.clone();
        let manager = person.manager.clone();
        let tool = person.tool.clone();
        let lead = person.lead;
        let family = person.family;
        let pets = person.pets.clone();
        let vehicle = person.vehicle;
        let conference = person.conference;
        let trip = person.trip;
        let family_structured = person.family_structured;
        let lead_structured = person.lead_structured;

        // Residence: the first move is a plain note that already knows its end
        // date, the current one is structured so the graph holds the truth.
        let city_a = world.cities[home[0].value].clone();
        let city_b = world.cities[home[1].value].clone();
        let plain = match rng.below(3) {
            0 => format!("{name} has settled in {city_a}."),
            1 => format!("{name} is renting an apartment in {city_a}."),
            _ => format!("After the move, {name} lives in {city_a}."),
        };
        push_mem(
            world,
            rng,
            plain,
            "note",
            "residence",
            home[0].at,
            Some(home[1].at),
            Vec::new(),
            Vec::new(),
            Some(FactId::Person(index, Attr::Home, 0)),
        );
        let structured = match rng.below(4) {
            0 => format!("{name} now lives in {city_b}."),
            1 => format!("{name} moved to {city_b} in {}.", month_year(home[1].at)),
            2 => format!("{name} relocated to {city_b}."),
            _ => format!("{name}'s home is in {city_b}."),
        };
        push_mem(
            world,
            rng,
            structured,
            "fact",
            "residence",
            home[1].at,
            None,
            vec![ent(&name, "person"), ent(&city_b, "place")],
            vec![rel(
                &name,
                "lives in",
                &city_b,
                true,
                &format!("{name} lives in {city_b}"),
            )],
            Some(FactId::Person(index, Attr::Home, 1)),
        );

        // Employment.
        let org_a = world.orgs[job[0].value].clone();
        let org_b = world.orgs[job[1].value].clone();
        let plain = match rng.below(3) {
            0 => format!(
                "{name} was working at {org_a} until {}.",
                month_year(job[1].at)
            ),
            1 => format!("{name} spent last year at {org_a}."),
            _ => format!("{name} had a role at {org_a}."),
        };
        push_mem(
            world,
            rng,
            plain,
            "note",
            "employment",
            job[0].at,
            Some(job[1].at),
            Vec::new(),
            Vec::new(),
            Some(FactId::Person(index, Attr::Job, 0)),
        );
        let structured = match rng.below(3) {
            0 => format!("{name} works at {org_b}."),
            1 => format!("{name} joined {org_b} in {}.", month_year(job[1].at)),
            _ => format!("{name} started a new job at {org_b}."),
        };
        push_mem(
            world,
            rng,
            structured,
            "fact",
            "employment",
            job[1].at,
            None,
            vec![ent(&name, "person"), ent(&org_b, "org")],
            vec![rel(
                &name,
                "works at",
                &org_b,
                true,
                &format!("{name} works at {org_b}"),
            )],
            Some(FactId::Person(index, Attr::Job, 1)),
        );

        // Manager: earlier ones are notes, the current one is structured.
        for (ordinal, change) in manager.iter().enumerate() {
            let manager_name = world.people[change.value].name.clone();
            let last = ordinal + 1 == manager.len();
            if last {
                let content = match rng.below(3) {
                    0 => format!("{name} reports to {manager_name}."),
                    1 => format!("{manager_name} is {name}'s manager."),
                    _ => format!("{name}'s manager is {manager_name}."),
                };
                push_mem(
                    world,
                    rng,
                    content,
                    "fact",
                    "management",
                    change.at,
                    None,
                    vec![ent(&name, "person"), ent(&manager_name, "person")],
                    vec![rel(
                        &name,
                        "has manager",
                        &manager_name,
                        true,
                        &format!("{name}'s manager is {manager_name}"),
                    )],
                    Some(FactId::Person(index, Attr::Manager, ordinal)),
                );
            } else {
                let content = match rng.below(2) {
                    0 => format!("{name} used to report to {manager_name}."),
                    _ => format!("{name} answered to {manager_name} earlier this year."),
                };
                let end = manager[ordinal + 1].at;
                push_mem(
                    world,
                    rng,
                    content,
                    "note",
                    "management",
                    change.at,
                    Some(end),
                    Vec::new(),
                    Vec::new(),
                    Some(FactId::Person(index, Attr::Manager, ordinal)),
                );
            }
        }

        // Preferred tool.
        for (ordinal, change) in tool.iter().enumerate() {
            let tool_name = world.tools[change.value].clone();
            let last = ordinal + 1 == tool.len();
            if last {
                let content = match rng.below(3) {
                    0 => format!("{name} prefers {tool_name} for most work."),
                    1 => format!("{name} now uses {tool_name} as the main tool."),
                    _ => format!("{name} switched to {tool_name}."),
                };
                push_mem(
                    world,
                    rng,
                    content,
                    "preference",
                    "tooling",
                    change.at,
                    None,
                    vec![ent(&name, "person"), ent(&tool_name, "tool")],
                    vec![rel(
                        &name,
                        "prefers",
                        &tool_name,
                        true,
                        &format!("{name} prefers {tool_name}"),
                    )],
                    Some(FactId::Person(index, Attr::Tool, ordinal)),
                );
            } else {
                let content = format!("{name} used to prefer {tool_name}.");
                let end = tool[ordinal + 1].at;
                push_mem(
                    world,
                    rng,
                    content,
                    "note",
                    "tooling",
                    change.at,
                    Some(end),
                    Vec::new(),
                    Vec::new(),
                    Some(FactId::Person(index, Attr::Tool, ordinal)),
                );
            }
        }

        // Project lead (only the people who lead one).
        if let Some(lead) = lead {
            let project = world.projects[lead.value].clone();
            if lead_structured {
                let content = match rng.below(3) {
                    0 => format!("{name} leads the {project} project."),
                    1 => format!("{project} is led by {name}."),
                    _ => format!("{name} took over as lead of {project}."),
                };
                push_mem(
                    world,
                    rng,
                    content,
                    "fact",
                    "project",
                    lead.at,
                    None,
                    vec![ent(&name, "person"), ent(&project, "project")],
                    vec![rel(
                        &name,
                        "leads",
                        &project,
                        true,
                        &format!("{name} leads {project}"),
                    )],
                    Some(FactId::Person(index, Attr::Lead, 0)),
                );
            } else {
                let content = match rng.below(2) {
                    0 => format!("{name} is running the {project} project this year."),
                    _ => format!(
                        "{name} has been leading {project} since {}.",
                        month_year(lead.at)
                    ),
                };
                push_mem(
                    world,
                    rng,
                    content,
                    "note",
                    "project",
                    lead.at,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Some(FactId::Person(index, Attr::Lead, 0)),
                );
            }
        }

        // Family: structured for people who are the anchor of a 3-hop query
        // (every other person), plain otherwise.
        if let Some((relative, kind)) = family {
            let relative_name = world.people[relative].name.clone();
            let relative_female = world.people[relative].female;
            let word = match (kind, relative_female) {
                ("sibling", true) => "sister",
                ("sibling", false) => "brother",
                (other, _) => other,
            };
            let at = start * DAY + rng.between(0, 300) * DAY + 11 * 3600;
            let structured = family_structured;
            if structured {
                let content = match rng.below(3) {
                    0 => format!("{name}'s {word} is {relative_name}."),
                    1 => format!("{relative_name} is {name}'s {word}."),
                    _ => format!("{name} and {relative_name} are siblings."),
                };
                let predicate = match kind {
                    "sibling" => "sibling of",
                    "parent" => "parent of",
                    _ => "spouse of",
                };
                push_mem(
                    world,
                    rng,
                    content,
                    "fact",
                    "family",
                    at,
                    None,
                    vec![ent(&name, "person"), ent(&relative_name, "person")],
                    vec![rel(
                        &name,
                        predicate,
                        &relative_name,
                        false,
                        &format!("{name}'s {word} is {relative_name}"),
                    )],
                    Some(FactId::Person(index, Attr::Family, 0)),
                );
            } else {
                let content = match rng.below(3) {
                    0 => format!("{name}'s {word} is {relative_name}. {name} calls {relative_name} every Sunday."),
                    1 => format!("{relative_name} is {name}'s {word}."),
                    _ => format!("{name} and {relative_name} are family."),
                };
                push_mem(
                    world,
                    rng,
                    content,
                    "note",
                    "family",
                    at,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Some(FactId::Person(index, Attr::Family, 0)),
                );
            }
        }

        // Pets: one memory naming all of them.
        if !pets.is_empty() {
            let names: Vec<String> = pets.iter().map(|p| world.pets[*p].clone()).collect();
            let at = start * DAY + rng.between(20, 340) * DAY + 19 * 3600;
            let content = if names.len() == 1 {
                match rng.below(3) {
                    0 => format!("{name} has a pet named {}.", names[0]),
                    1 => format!("{name}'s pet is {}.", names[0]),
                    _ => format!("{name} adopted {}.", names[0]),
                }
            } else {
                format!("{name} has two pets: {} and {}.", names[0], names[1])
            };
            push_mem(
                world,
                rng,
                content,
                "note",
                "pets",
                at,
                None,
                Vec::new(),
                Vec::new(),
                Some(FactId::Person(index, Attr::Pet, 0)),
            );
        }

        // Vehicle.
        if let Some(vehicle) = vehicle {
            let model = world.vehicles[vehicle].clone();
            let at = start * DAY + rng.between(10, 330) * DAY + 15 * 3600;
            let content = match rng.below(3) {
                0 => format!("{name} drives a {model}."),
                1 => format!("{name} owns a {model}."),
                _ => format!("{name} bought a {model}."),
            };
            push_mem(
                world,
                rng,
                content,
                "note",
                "vehicle",
                at,
                None,
                Vec::new(),
                Vec::new(),
                Some(FactId::Person(index, Attr::Vehicle, 0)),
            );
        }

        // Conference.
        if let Some((at, conference, city)) = conference {
            let conference = world.conferences[conference].clone();
            let city = world.cities[city].clone();
            let content = match rng.below(3) {
                0 => format!("{name} attended {conference} in {city} on {}.", date(at)),
                1 => format!("{name} was at {conference} ({city}) on {}.", date(at)),
                _ => format!(
                    "{name} gave a talk at {conference} in {city} on {}. {first} stayed for the workshops.",
                    date(at)
                ),
            };
            push_mem(
                world,
                rng,
                content,
                "event",
                "conference",
                at,
                None,
                Vec::new(),
                Vec::new(),
                Some(FactId::Person(index, Attr::Conference, 0)),
            );
        }

        // Trip.
        if let Some((at, city)) = trip {
            let city = world.cities[city].clone();
            let content = match rng.below(3) {
                0 => format!("{name} travelled to {city} for a customer visit."),
                1 => format!("{name} visited {city} on a business trip."),
                _ => format!("{name} spent a week in {city} for work."),
            };
            push_mem(
                world,
                rng,
                content,
                "event",
                "travel",
                at,
                None,
                Vec::new(),
                Vec::new(),
                Some(FactId::Person(index, Attr::Trip, 0)),
            );
        }

        let _ = (today, first);
    }
}

// ------------------------------------------------------------------ queries

fn queries(world: &World, keys: &HashMap<FactId, String>, count: usize, seed: u64) -> Vec<Value> {
    let mut rng = Rng::new(seed ^ 0xC0FF_EE00 ^ count as u64);
    let people = &world.people;
    let n = people.len();

    // Split the budget across families, then interleave so `--limit-queries`
    // still sees every family.
    let share = |fraction: usize| (count * fraction / 100).max(1);
    let quotas = [
        ("current", share(17)),
        ("as-of", share(17)),
        ("history", share(13)),
        ("two-hop", share(20)),
        ("three-hop", share(13)),
        ("temporal-window", share(10)),
        ("aggregation", share(10)),
    ];
    let mut families: Vec<Vec<Value>> = Vec::new();
    for (name, quota) in quotas {
        let mut out = Vec::new();
        let mut guard = 0;
        while out.len() < quota && guard < quota * 40 {
            guard += 1;
            let index = rng.below(n);
            let query = match name {
                "current" => current_query(world, keys, index, &mut rng),
                "as-of" => as_of_query(world, keys, index, &mut rng),
                "history" => history_query(world, keys, index, &mut rng),
                "two-hop" => two_hop_query(world, keys, index, &mut rng),
                "three-hop" => three_hop_query(world, keys, index, &mut rng),
                "temporal-window" => temporal_query(world, keys, index, &mut rng),
                _ => aggregation_query(world, keys, index, &mut rng),
            };
            if let Some(query) = query {
                out.push(query);
            }
        }
        families.push(out);
    }
    let mut out = Vec::new();
    let longest = families.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..longest {
        for family in &families {
            if let Some(query) = family.get(i) {
                out.push(query.clone());
            }
        }
    }
    out.truncate(count);
    out
}

fn key(keys: &HashMap<FactId, String>, fact: FactId) -> Option<String> {
    keys.get(&fact).cloned()
}

fn current_query(
    world: &World,
    keys: &HashMap<FactId, String>,
    index: usize,
    rng: &mut Rng,
) -> Option<Value> {
    let person = &world.people[index];
    let name = &person.name;
    match rng.below(4) {
        0 => {
            let last = person.home.len() - 1;
            let city = &world.cities[person.home[last].value];
            let expect = key(keys, FactId::Person(index, Attr::Home, last))?;
            let mut absent = Vec::new();
            if last > 0 {
                if let Some(old) = key(keys, FactId::Person(index, Attr::Home, last - 1)) {
                    absent.push(old);
                }
            }
            let q = match rng.below(4) {
                0 => format!("where does {name} live now"),
                1 => format!("which city is {name} based in"),
                2 => format!("what is the current city of {name}"),
                _ => format!("where is {name} living at the moment"),
            };
            Some(json!({
                "q": q, "category": "current", "expect": [expect],
                "expect_absent": absent, "answers": [city],
                "note": "current residence; the previous city must not outrank it",
            }))
        }
        1 => {
            let last = person.job.len() - 1;
            let org = &world.orgs[person.job[last].value];
            let expect = key(keys, FactId::Person(index, Attr::Job, last))?;
            let mut absent = Vec::new();
            if let Some(old) = key(keys, FactId::Person(index, Attr::Job, last - 1)) {
                absent.push(old);
            }
            let q = match rng.below(3) {
                0 => format!("where does {name} work now"),
                1 => format!("which company does {name} work for"),
                _ => format!("who employs {name} currently"),
            };
            Some(json!({
                "q": q, "category": "current", "expect": [expect],
                "expect_absent": absent, "answers": [org],
                "note": "current employer",
            }))
        }
        2 => {
            let last = person.manager.len() - 1;
            let manager = &world.people[person.manager[last].value].name;
            let expect = key(keys, FactId::Person(index, Attr::Manager, last))?;
            let q = match rng.below(3) {
                0 => format!("who is {name}'s manager"),
                1 => format!("who does {name} report to"),
                _ => format!("who manages {name} at work"),
            };
            Some(json!({
                "q": q, "category": "current", "expect": [expect],
                "answers": [manager], "note": "current manager",
            }))
        }
        _ => {
            let last = person.tool.len() - 1;
            let tool = &world.tools[person.tool[last].value];
            let expect = key(keys, FactId::Person(index, Attr::Tool, last))?;
            let q = match rng.below(3) {
                0 => format!("what tool does {name} prefer"),
                1 => format!("which tool does {name} use most"),
                _ => format!("what is {name}'s preferred tool now"),
            };
            Some(json!({
                "q": q, "category": "current", "expect": [expect],
                "answers": [tool], "note": "current preferred tool",
            }))
        }
    }
}

fn as_of_query(
    world: &World,
    keys: &HashMap<FactId, String>,
    index: usize,
    rng: &mut Rng,
) -> Option<Value> {
    let person = &world.people[index];
    let name = &person.name;
    let (first, second) = (person.home[0], person.home[1]);
    if second.at - first.at < 40 * DAY {
        return None;
    }
    // Noon in the middle of the first interval: unambiguous for `as_of`, and
    // the month named in the question is that of the instant asked about.
    let middle = first.at + (second.at - first.at) / 2;
    let at = middle.div_euclid(DAY) * DAY + 12 * 3600;
    if at < first.at + 7 * DAY || at > second.at - 7 * DAY {
        return None;
    }
    let city = &world.cities[first.value];
    let expect = key(keys, FactId::Person(index, Attr::Home, 0))?;
    let current = key(keys, FactId::Person(index, Attr::Home, 1))?;
    let month = month_year(at);
    let q = match rng.below(3) {
        0 => format!("where did {name} live in {month}"),
        1 => format!("which city was {name} living in during {month}"),
        _ => format!("what was {name}'s city in {month}"),
    };
    Some(json!({
        "q": q, "category": "as-of", "expect": [expect],
        "expect_absent": [current], "as_of": date(at), "answers": [city],
        "note": "residence at a past instant; the later move must not appear",
    }))
}

fn history_query(
    world: &World,
    keys: &HashMap<FactId, String>,
    index: usize,
    rng: &mut Rng,
) -> Option<Value> {
    let person = &world.people[index];
    let name = &person.name;
    let mut expect = Vec::new();
    let mut answers: Vec<String> = Vec::new();
    for (ordinal, change) in person.job.iter().enumerate() {
        expect.push(key(keys, FactId::Person(index, Attr::Job, ordinal))?);
        let org = world.orgs[change.value].clone();
        if !answers.contains(&org) {
            answers.push(org);
        }
    }
    if answers.len() < 2 {
        return None;
    }
    let q = match rng.below(3) {
        0 => format!("which companies has {name} worked for"),
        1 => format!("where has {name} worked"),
        _ => format!("list the employers {name} has had"),
    };
    Some(json!({
        "q": q, "category": "history", "expect": expect, "include_history": true,
        "answers": answers, "note": "every employer, including the previous one",
    }))
}

fn two_hop_query(
    world: &World,
    keys: &HashMap<FactId, String>,
    index: usize,
    rng: &mut Rng,
) -> Option<Value> {
    let person = &world.people[index];
    let name = &person.name;
    if rng.chance(50) {
        let last = person.manager.len() - 1;
        let manager_index = person.manager[last].value;
        let manager = &world.people[manager_index];
        let manager_home = manager.home.len() - 1;
        let city = &world.cities[manager.home[manager_home].value];
        let expect = vec![
            key(keys, FactId::Person(index, Attr::Manager, last))?,
            key(
                keys,
                FactId::Person(manager_index, Attr::Home, manager_home),
            )?,
        ];
        let q = match rng.below(3) {
            0 => format!("which city does {name}'s manager live in"),
            1 => format!("where does the manager of {name} live"),
            _ => format!("what city is the home of {name}'s manager"),
        };
        Some(json!({
            "q": q, "category": "two-hop", "expect": expect, "answers": [city],
            "note": "person -> manager -> city",
        }))
    } else {
        let project_index = person.lead?.value;
        if !person.lead_structured || world.current_leads[project_index] != index {
            return None;
        }
        let language = &world.tools[world.project_language[project_index]];
        let expect = vec![
            key(keys, FactId::Person(index, Attr::Lead, 0))?,
            key(keys, FactId::Project(project_index))?,
        ];
        let q = match rng.below(3) {
            0 => format!("what language does the project {name} leads use"),
            1 => format!("which language is used by {name}'s project"),
            _ => format!("what is the main language of the project led by {name}"),
        };
        Some(json!({
            "q": q, "category": "two-hop", "expect": expect, "answers": [language],
            "note": "person -> project -> language",
        }))
    }
}

fn three_hop_query(
    world: &World,
    keys: &HashMap<FactId, String>,
    index: usize,
    rng: &mut Rng,
) -> Option<Value> {
    let person = &world.people[index];
    let name = person.name.clone();
    let (sibling_index, kind) = person.family?;
    if kind != "sibling" || !person.family_structured {
        return None;
    }
    let sibling = &world.people[sibling_index];
    let last = sibling.manager.len() - 1;
    let manager_index = sibling.manager[last].value;
    if manager_index == index || manager_index == sibling_index {
        return None;
    }
    let manager = &world.people[manager_index];
    let manager_home = manager.home.len() - 1;
    let city = &world.cities[manager.home[manager_home].value];
    let word = if sibling.female { "sister" } else { "brother" };
    let expect = vec![
        key(keys, FactId::Person(index, Attr::Family, 0))?,
        key(keys, FactId::Person(sibling_index, Attr::Manager, last))?,
        key(
            keys,
            FactId::Person(manager_index, Attr::Home, manager_home),
        )?,
    ];
    let q = match rng.below(3) {
        0 => format!("which city does the manager of {name}'s {word} live in"),
        1 => format!("where does the manager of {name}'s {word} live"),
        _ => format!("what city is home to the manager of {name}'s {word}"),
    };
    Some(json!({
        "q": q, "category": "three-hop", "expect": expect, "answers": [city],
        "note": "person -> sibling -> manager -> city",
    }))
}

fn temporal_query(
    world: &World,
    keys: &HashMap<FactId, String>,
    index: usize,
    rng: &mut Rng,
) -> Option<Value> {
    let person = &world.people[index];
    let (at, conference, city) = person.conference?;
    // Only the people whose conference is in the previous calendar month.
    if !(NOW - 50 * DAY..=NOW - 25 * DAY).contains(&at) {
        return None;
    }
    let conference = &world.conferences[conference];
    let city = &world.cities[city];
    let expect = key(keys, FactId::Person(index, Attr::Conference, 0))?;
    let name = &person.name;
    if rng.chance(50) {
        let q = match rng.below(2) {
            0 => format!("which conference did {name} attend last month"),
            _ => format!("what conference was {name} at last month"),
        };
        Some(json!({
            "q": q, "category": "temporal-window", "expect": [expect],
            "answers": [conference], "note": "event inside last month",
        }))
    } else {
        let q = format!("which city hosted the conference {name} attended last month");
        Some(json!({
            "q": q, "category": "temporal-window", "expect": [expect],
            "answers": [city], "note": "city of last month's conference",
        }))
    }
}

fn aggregation_query(
    world: &World,
    keys: &HashMap<FactId, String>,
    index: usize,
    rng: &mut Rng,
) -> Option<Value> {
    let person = &world.people[index];
    if person.pets.is_empty() {
        return None;
    }
    let expect = key(keys, FactId::Person(index, Attr::Pet, 0))?;
    let answers: Vec<String> = person.pets.iter().map(|p| world.pets[*p].clone()).collect();
    let name = &person.name;
    let q = match rng.below(3) {
        0 => format!("which pets does {name} have"),
        1 => format!("what are the names of {name}'s pets"),
        _ => format!("list {name}'s pets"),
    };
    Some(json!({
        "q": q, "category": "aggregation", "expect": [expect],
        "answers": answers, "note": "all pets in one memory",
    }))
}

// ------------------------------------------------------------------ main

fn usage() -> String {
    "usage: gen_world --people N [--seed S] [--queries Q] --out FILE".into()
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut people = 300usize;
    let mut seed = 7u64;
    let mut query_count = 600usize;
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        let take = |i: &mut usize| -> Result<String, String> {
            *i += 1;
            args.get(*i).cloned().ok_or_else(usage)
        };
        match args[i].as_str() {
            "--people" => {
                people = take(&mut i)?
                    .parse()
                    .map_err(|_| "--people needs a number".to_string())?
            }
            "--seed" => {
                seed = take(&mut i)?
                    .parse()
                    .map_err(|_| "--seed needs a number".to_string())?
            }
            "--queries" => {
                query_count = take(&mut i)?
                    .parse()
                    .map_err(|_| "--queries needs a number".to_string())?
            }
            "--out" => out = Some(PathBuf::from(take(&mut i)?)),
            other => return Err(format!("unknown argument '{other}'\n{}", usage())),
        }
        i += 1;
    }
    let out = out.ok_or_else(usage)?;
    if people == 0 {
        return Err("--people must be positive".into());
    }

    let mut rng = Rng::new(seed);
    let mut world = build(&mut rng, people);

    // Sort chronologically, then number the memories in that order.
    let mut mems = std::mem::take(&mut world.mems);
    mems.sort_by_key(|m| (m.at, m.id));
    let mut keys: HashMap<FactId, String> = HashMap::new();
    let mut ordered = Vec::with_capacity(mems.len());
    for (position, mem) in mems.iter().enumerate() {
        let key = format!("w{:06}", position + 1);
        if let Some(fact) = mem.fact {
            keys.insert(fact, key.clone());
        }
        let mut value = mem.value.clone();
        value["key"] = json!(key);
        ordered.push(value);
    }

    let queries = queries(&world, &keys, query_count, seed);
    let document = json!({
        "now": "2026-10-21T15:00:00Z",
        "memories": ordered,
        "queries": queries,
    });
    let text = serde_json::to_string(&document).map_err(|e| e.to_string())?;
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&out, text.as_bytes()).map_err(|e| e.to_string())?;

    let structured = ordered
        .iter()
        .filter(|m| m.get("relations").is_some())
        .count();
    eprintln!(
        "gen_world: {} people, {} memories ({} structured, {:.0}%), {} queries, {} bytes -> {}",
        people,
        ordered.len(),
        structured,
        100.0 * structured as f64 / ordered.len().max(1) as f64,
        queries.len(),
        text.len(),
        out.display()
    );
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("gen_world: {e}");
        std::process::exit(1);
    }
}
