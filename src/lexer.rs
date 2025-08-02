use rust_stemmers::{Algorithm, Stemmer};

/// A simple lexer for tokenizing text. It supports numeric, alphabetic, and
/// other characters, and applies English stemming to alphabetic tokens.
pub struct Lexer<'a> {
    /// The input text as a slice of characters.
    pub input: &'a [char],
}

impl<'a> Lexer<'a> {
    /// Creates a new `Lexer` instance.
    ///
    /// # Arguments
    /// * `input` - The input text as a slice of characters.
    pub fn new(input: &'a [char]) -> Self {
        Self { input }
    }

    /// Trims whitespace from the left side of the input.
    fn trim_left(&mut self) {
        while !self.input.is_empty() && self.input[0].is_whitespace() {
            self.input = &self.input[1..];
        }
    }

    /// Chops `n` characters from the beginning of the input and returns them
    /// as a slice.
    ///
    /// # Arguments
    /// * `n` - The number of characters to chop.
    ///
    /// # Returns
    /// A slice of characters representing the chopped token.
    fn chop(&mut self, n: usize) -> &'a [char] {
        let token = &self.input[0..n];
        self.input = &self.input[n..];
        token
    }

    /// Chops characters from the input while a given predicate remains true.
    ///
    /// # Arguments
    /// * `predicate` - A closure that takes a character and returns `true` if
    ///   it should be included.
    ///
    /// # Returns
    /// A slice of characters representing the chopped token.
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

    /// Extracts the next token from the input. It handles numeric tokens,
    /// alphabetic tokens (with stemming), and single-character tokens.
    ///
    /// # Returns
    /// An `Option` containing the next token as a `String`, or `None` if no
    /// more tokens are available.
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

    /// Stems a given token using the English Porter2 stemming algorithm.
    ///
    /// # Arguments
    /// * `token` - The token to stem.
    ///
    /// # Returns
    /// The stemmed version of the token as a `String`.
    fn stem_token(&self, token: &str) -> String {
        let stemmer = Stemmer::create(Algorithm::English);
        stemmer.stem(token).to_string()
    }

    /// Retrieves all tokens from the input, applying stemming and removing
    /// specified stop words.
    ///
    /// # Arguments
    /// * `stop_words` - A slice of `String`s representing words to be filtered
    ///   out.
    ///
    /// # Returns
    /// A `Vec` of processed tokens as `String`s.
    pub fn get_tokens(&mut self, stop_words: &[String]) -> Vec<String> {
        let mut tokens = Vec::new();
        for token in self.by_ref() {
            tokens.push(token);
        }

        self.remove_stop_words(&mut tokens, stop_words);
        tokens
    }

    /// Removes specified stop words from a mutable vector of tokens.
    ///
    /// # Arguments
    /// * `tokens` - A mutable reference to the `Vec<String>` of tokens.
    /// * `stop_words` - A slice of `String`s representing stop words.
    fn remove_stop_words(&self, tokens: &mut Vec<String>, stop_words: &[String]) {
        *tokens = tokens
            .iter()
            .filter(|t| !stop_words.contains(t))
            .map(|t| t.to_string())
            .collect::<Vec<String>>();
    }
}

impl Iterator for Lexer<'_> {
    type Item = String;

    /// Implements the `Iterator` trait for `Lexer`, allowing it to be used in
    /// loops.
    fn next(&mut self) -> Option<Self::Item> {
        self.next_token()
    }
}
