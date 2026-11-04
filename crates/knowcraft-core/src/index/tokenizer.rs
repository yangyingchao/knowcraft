//! jieba 中文分词接入 tantivy。

use std::sync::OnceLock;

use jieba_rs::{Jieba, TokenizeMode};
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};

static JIEBA: OnceLock<Jieba> = OnceLock::new();

fn jieba() -> &'static Jieba {
    // 词典在首次分词时加载，避免拖慢 Index::open 所在的主线程。
    JIEBA.get_or_init(Jieba::new)
}

#[derive(Clone, Default)]
pub(crate) struct JiebaTokenizer;

impl Tokenizer for JiebaTokenizer {
    type TokenStream<'a> = JiebaTokenStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let tokens = jieba()
            .tokenize(text, TokenizeMode::Search, true)
            .into_iter()
            .filter(|token| !token.word.trim().is_empty())
            .enumerate()
            .map(|(position, token)| Token {
                offset_from: token.byte_start,
                offset_to: token.byte_end,
                position,
                text: token.word.to_string(),
                position_length: 1,
            })
            .collect();
        JiebaTokenStream { tokens, index: 0 }
    }
}

pub(crate) struct JiebaTokenStream {
    tokens: Vec<Token>,
    index: usize,
}

impl TokenStream for JiebaTokenStream {
    fn advance(&mut self) -> bool {
        if self.index < self.tokens.len() {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn token(&self) -> &Token {
        &self.tokens[self.index - 1]
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.index - 1]
    }
}
