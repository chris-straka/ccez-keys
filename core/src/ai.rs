//! Muse (or another OpenAI-compatible provider) for corrections while a
//! learning language is typed, and for Ask: a chat about something typed.
//! The key comes in with each call (the app keeps it in the Android
//! Keystore) and is never logged or stored here.

use ccez_lang_core::ai::{self, Message};
use ccez_lang_core::correction::{self, Op};

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum FixKind {
    Same,
    Del,
    Ins,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FixPart {
    pub kind: FixKind,
    pub text: String,
}

/// A correction: the corrected text, why in one line, and the word diff
/// (crossed out what goes, green what comes in), as ccez-llm shows it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Fix {
    pub text: String,
    pub why: String,
    pub parts: Vec<FixPart>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ChatMessage {
    /// "system", "user" or "assistant".
    pub role: String,
    pub text: String,
}

/// Streamed answers reach the app through this.
#[uniffi::export(callback_interface)]
pub trait AnswerSink: Send + Sync {
    /// The answer so far.
    fn text(&self, so_far: String);
}

pub struct Provider {
    pub url: String,
    pub model: String,
    pub thinking: Option<String>,
}

pub fn provider(id: &str) -> Option<Provider> {
    let p = ai::provider(id)?;
    if p.url.is_empty() {
        return None;
    }
    // Corrections want speed: the least thinking the provider allows.
    Some(Provider { url: p.url.into(), model: p.model.into(), thinking: p.thinking.first().map(|t| t.to_string()) })
}

pub fn correction_messages(lang: &str, text: &str, explain_in: &str) -> Vec<Message> {
    correction::correction_messages(lang, text, explain_in, None)
}

pub fn to_fix(written: &str, answer: &str) -> Option<Fix> {
    let c = correction::parse_correction(written, answer)?;
    let parts = c
        .ops
        .into_iter()
        .map(|o| match o {
            Op::Same { text } => FixPart { kind: FixKind::Same, text },
            Op::Del { text } => FixPart { kind: FixKind::Del, text },
            Op::Ins { text } => FixPart { kind: FixKind::Ins, text },
        })
        .collect();
    Some(Fix { text: c.text, why: c.why.unwrap_or_default(), parts })
}

pub fn to_messages(v: &[ChatMessage]) -> Vec<Message> {
    v.iter().map(|m| Message { role: m.role.clone(), content: m.text.clone() }).collect()
}

/// The opening of an Ask chat about `typed` (what the owner just wrote),
/// with their question.
pub fn ask_start(typed: &str, question: &str) -> Vec<ChatMessage> {
    ai::ask_messages(typed, typed, Some(question), false)
        .into_iter()
        .map(|m| ChatMessage { role: m.role, text: m.content })
        .collect()
}
