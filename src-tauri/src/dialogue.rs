//! The launcher's conversation, kept in memory only: the turns answered since
//! the last «new chat» or cold start. A turn finished after the session was
//! cleared or replaced must not join the new one, hence the generation.

use std::sync::{Mutex, PoisonError};

use crate::bridge::{AskAnswer, Hit};

/// One answered question. `hits` is every passage the turn's own search found,
/// `None` when the turn answered from the sources of an earlier one.
#[derive(Debug, Clone)]
pub struct SessionTurn {
    pub query: String,
    pub answer: AskAnswer,
    pub hits: Option<Vec<Hit>>,
}

#[derive(Default)]
pub struct Session {
    inner: Mutex<(u64, Vec<SessionTurn>)>,
}

impl Session {
    fn lock(&self) -> std::sync::MutexGuard<'_, (u64, Vec<SessionTurn>)> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The generation and the turns a question starts from.
    pub fn snapshot(&self) -> (u64, Vec<SessionTurn>) {
        self.lock().clone()
    }

    /// Adds `turn` unless the session changed since `generation` was read, and
    /// answers with the session's turns right after the push (taken under the
    /// same lock, so a later clear cannot slip in between); `None` when refused.
    pub fn push_if(&self, generation: u64, turn: SessionTurn) -> Option<Vec<SessionTurn>> {
        let mut guard = self.lock();
        if guard.0 != generation {
            return None;
        }
        guard.1.push(turn);
        Some(guard.1.clone())
    }

    pub fn clear(&self) {
        self.replace(Vec::new());
    }

    pub fn replace(&self, turns: Vec<SessionTurn>) {
        let mut guard = self.lock();
        guard.0 += 1;
        guard.1 = turns;
    }

    /// The question that opened the session, which names it in the history.
    pub fn first_query(&self) -> Option<String> {
        self.lock().1.first().map(|t| t.query.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::{ContentArmReport, RefusalKind, TextArmReport};

    fn turn(query: &str) -> SessionTurn {
        SessionTurn {
            query: query.into(),
            answer: AskAnswer::Refused {
                kind: RefusalKind::NoCandidates,
                text: TextArmReport::Off,
                content: ContentArmReport::NoKey,
            },
            hits: None,
        }
    }

    #[test]
    fn a_turn_finished_after_a_clear_does_not_join_the_new_session() {
        let session = Session::default();
        let (old, _) = session.snapshot();
        session.clear();
        assert!(session.push_if(old, turn("late")).is_none());
        assert!(session.snapshot().1.is_empty());
        let (now, _) = session.snapshot();
        assert!(session.push_if(now, turn("fresh")).is_some());
        assert_eq!(session.first_query().as_deref(), Some("fresh"));
    }

    #[test]
    fn replacing_the_session_also_closes_the_door_on_a_running_turn() {
        let session = Session::default();
        let (old, _) = session.snapshot();
        session.replace(vec![turn("kept")]);
        assert!(session.push_if(old, turn("late")).is_none());
        assert_eq!(session.snapshot().1.len(), 1);
    }

    #[test]
    fn a_push_answers_with_the_turns_it_made_and_none_once_refused() {
        let session = Session::default();
        let (generation, _) = session.snapshot();
        assert!(session.push_if(generation, turn("one")).is_some());
        let after = session.push_if(generation, turn("two")).unwrap();
        let queries: Vec<&str> = after.iter().map(|t| t.query.as_str()).collect();
        assert_eq!(queries, ["one", "two"]);
        session.clear();
        assert!(session.push_if(generation, turn("late")).is_none());
    }
}
