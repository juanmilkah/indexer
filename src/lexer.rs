use rust_stemmers::{Algorithm, Stemmer};

pub struct Lexer<'a> {
    pub input: &'a [char],
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a [char]) -> Self {
        Self { input }
    }

    fn trim_left(&mut self) {
        while !self.input.is_empty() && self.input[0].is_whitespace() {
            self.input = &self.input[1..];
        }
    }

    fn chop(&mut self, n: usize) -> &'a [char] {
        let token = &self.input[0..n];
        self.input = &self.input[n..];
        token
    }

    fn chop_while<P>(&mut self, mut predicate: P) -> &'a [char]
    where
        P: FnMut(&char) -> bool,
    {
        let mut n = 0;
        while n < self.input.len() && predicate(&self.input[n]) {
            n += 1;
        }

        self.chop(n)
    }

    fn next_token(&mut self) -> Option<String> {
        self.trim_left();

        if self.input.is_empty() {
            return None;
        }

        if self.input[0].is_numeric() {
            return Some(self.chop_while(|x| x.is_numeric()).iter().collect());
        }

        if self.input[0].is_alphabetic() {
            let term: String = self.chop_while(|x| x.is_alphanumeric()).iter().collect();

            let stemmed_token = self.stem_token(&term);
            return Some(stemmed_token);
        }
        Some(self.chop(1).iter().collect())
    }

    fn stem_token(&self, token: &str) -> String {
        let stemmer = Stemmer::create(Algorithm::English);
        stemmer.stem(token).to_string()
    }

    pub fn get_tokens(&mut self, stop_words: &[String]) -> Vec<String> {
        let mut tokens = Vec::new();
        while let Some(token) = self.next() {
            tokens.push(token);
        }

        self.remove_stop_words(&mut tokens, stop_words);
        tokens
    }

    fn remove_stop_words(&self, tokens: &mut Vec<String>, stop_words: &[String]) {
        for i in 0..tokens.len() {
            if stop_words.contains(&tokens[i]) {
                tokens.remove(i);
            }
        }
    }
}

impl Iterator for Lexer<'_> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_token()
    }
}
