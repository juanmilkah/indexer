use std::collections::HashMap;

struct VectorCompare {}

impl VectorCompare {
    // count of every word that occurs in a document
    fn concodance(&self, document: String) -> HashMap<String, f32> {
        let mut map = HashMap::new();

        for word in document.split(" ") {
            let mut count: f32 = *map.entry(word.to_string()).or_insert(0.0);
            count += 1.0;

            map.insert(word.to_string(), count);
        }

        map
    }

    fn magnitude(&self, concodance: &HashMap<String, f32>) -> f32 {
        let mut total = 0.0;

        for (_, count) in concodance.iter() {
            total += count * count;
        }

        total.sqrt()
    }

    fn relation(
        &self,
        concodance_1: &HashMap<String, f32>,
        concodance_2: &HashMap<String, f32>,
    ) -> f32 {
        let mut top_value: f32 = 0.0;

        for (word, count) in concodance_1.iter() {
            if concodance_2.contains_key(word) {
                top_value += count * concodance_2.get(word).unwrap();
            }
        }

        let conc_1 = self.magnitude(concodance_1);
        let conc_2 = self.magnitude(concodance_2);

        if conc_1 * conc_2 != 0.0 {
            top_value / (conc_1 * conc_2)
        } else {
            0.0
        }
    }
}

fn main() {
    let documents = HashMap::from([
        (
            0,
            "At Scale You Will Hit Every Performance Issue I used to think I knew a bit about performance scalability and how to keep things trucking when you hit large amounts of data Truth is I know diddly squat on the subject since the most I have ever done is read about how its done To understand how I came about realising this you need some background".to_string(),
        ),
        (
            1,
            "Richard Stallman to visit Australia Im not usually one to promote events and the like unless I feel there is a genuine benefit to be had by attending but this is one stands out Richard M Stallman the guru of Free Software is coming Down Under to hold a talk You can read about him here Open Source Celebrity to visit Australia".to_string(),

        ),
        (
            2,
            "MySQL Backups Done Easily One thing that comes up a lot on sites like Stackoverflow and the like is how to backup MySQL databases The first answer is usually use mysqldump This is all fine and good till you start to want to dump multiple databases You can do this all in one like using the all databases option however this makes restoring a single database an issue since you have to parse out the parts you want which can be a pain".to_string(),
        ),
        (
            3,
            "Why You Shouldnt roll your own CAPTCHA At a TechEd I attended a few years ago I was watching a presentation about Security presented by Rocky Heckman read his blog its quite good In it he was talking about security algorithms The part that really stuck with me went like this".to_string(),
        ),
        (
            4,
            "The Great Benefit of Test Driven Development Nobody Talks About The feeling of productivity because you are writing lots of code Think about that for a moment Ask any developer who wants to develop why they became a developer One of the first things that comes up is I enjoy writing code This is one of the things that I personally enjoy doing Writing code any code especially when its solving my current problem makes me feel productive It makes me feel like Im getting somewhere Its empowering".to_string(),
        ),
        (
            5,
            "Setting up GIT to use a Subversion SVN style workflow Moving from Subversion SVN to GIT can be a little confusing at first I think the biggest thing I noticed was that GIT doesnt have a specific workflow you have to pick your own Personally I wanted to stick to my Subversion like work-flow with a central server which all my machines would pull and push too Since it took a while to set up I thought I would throw up a blog post on how to do it".to_string(),
        ),
        (
            6,
            "Why CAPTCHA Never Use Numbers 0 1 5 7 Interestingly this sort of question pops up a lot in my referring search term stats Why CAPTCHAs never use the numbers 0 1 5 7 Its a relativity simple question with a reasonably simple answer Its because each of the above numbers are easy to confuse with a letter See the below".to_string(),
        ),
    ]);

    let v = VectorCompare {};

    //build the index
    let mut index = HashMap::new();

    for (key, value) in documents.iter() {
        let conc = v.concodance(value.to_lowercase());
        index.insert(key, conc);
    }

    let search_term = "benefit security databases".to_string();
    let mut matches = Vec::new();

    for i in 0..index.len() {
        let relation = v.relation(
            &v.concodance(search_term.to_lowercase()),
            index.get(&i).unwrap(),
        );

        if relation != 0.0 {
            let doc = documents.get(&i).unwrap();
            matches.push((relation, doc));
        }
    }
    matches.sort_by_key(|k| k.0 as u32);
    matches.reverse();

    for m in matches {
        println!("{}:  {}\n\n", m.0, m.1);
    }
}
