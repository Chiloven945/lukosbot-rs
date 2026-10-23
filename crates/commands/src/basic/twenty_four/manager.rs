use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lukosbot_domain::{ChatAddress, PlatformKind, PlatformUserId};

use super::expression::{EPSILON, TARGET, evaluate};
use super::puzzle::generate;
use crate::basic::random::RandomSource;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameSessionKey {
    pub platform: PlatformKind,
    pub user_id: PlatformUserId,
}

#[derive(Debug, Clone)]
pub struct GameSession {
    pub address: ChatAddress,
    pub nums: [i32; 4],
    pub solution: String,
    pub expires_at: Instant,
}

#[derive(Debug, Clone)]
pub struct TimeoutNotice {
    pub address: ChatAddress,
    pub text: String,
}

pub struct GameManager {
    time_limit: Duration,
    prefix: String,
    random: Arc<dyn RandomSource>,
    sessions: HashMap<GameSessionKey, GameSession>,
}

impl GameManager {
    pub fn new(time_limit: Duration, random: Arc<dyn RandomSource>) -> Self {
        Self::with_prefix(time_limit, "/", random)
    }

    pub fn with_prefix(
        time_limit: Duration,
        prefix: impl Into<String>,
        random: Arc<dyn RandomSource>,
    ) -> Self {
        Self {
            time_limit,
            prefix: prefix.into(),
            random,
            sessions: HashMap::new(),
        }
    }

    pub fn start(&mut self, key: GameSessionKey, address: ChatAddress, now: Instant) -> String {
        if let Some(existing) = self.sessions.get(&key) {
            if now < existing.expires_at {
                return format!(
                    "你已经有一局 24 点游戏正在进行：\n数字：{}\n\n如果想放弃这一局，可以发送：{}24 giveup\n或直接发送表达式作为答案。",
                    format_nums(&existing.nums),
                    self.prefix
                );
            }
            self.sessions.remove(&key);
        }

        let puzzle = generate(self.random.as_ref());
        let session = GameSession {
            address,
            nums: puzzle.nums,
            solution: puzzle.solution,
            expires_at: now + self.time_limit,
        };
        let text = format!(
            "新的一局 24 点开始了（限时 {} 秒）！\n请使用下面 4 个数字，通过 + - * / 和括号算出 24：\n数字：{}\n\n提交答案：{}24 <表达式>\n放弃并看答案：{}24 giveup",
            self.time_limit.as_secs(),
            format_nums(&session.nums),
            self.prefix,
            self.prefix
        );
        self.sessions.insert(key, session);
        text
    }

    pub fn submit(&mut self, key: &GameSessionKey, input: &str, now: Instant) -> String {
        let input = input.trim();
        if input.is_empty() {
            return format!("请提交表达式，或发送 {}24 giveup 放弃。", self.prefix);
        }
        if input.eq_ignore_ascii_case("giveup") {
            return self.give_up(key, now);
        }

        let Some(session) = self.sessions.get(key).cloned() else {
            return no_game(&self.prefix);
        };
        if now >= session.expires_at {
            self.sessions.remove(key);
            return no_game(&self.prefix);
        }

        let evaluated = match evaluate(input) {
            Ok(evaluated) => evaluated,
            Err(error) => {
                return format!("无法解析表达式，请只使用数字、+ - * / 和括号。\n原因：{error}");
            }
        };

        if (evaluated.value - TARGET).abs() > EPSILON {
            return format!(
                "你的结果不是 24，而是：{}\n可以继续尝试，或发送 {}24 giveup 查看答案。",
                evaluated.value, self.prefix
            );
        }

        let mut expected = session.nums;
        expected.sort_unstable();
        let mut used = evaluated.numbers;
        used.sort_unstable();

        if expected.as_slice() != used.as_slice() {
            return format!(
                "虽然结果是 24，但你使用的数字与题目不一致。\n题目数字：{}\n你使用的数字：{}\n\n请只使用这 4 个数字各一次，再试一次吧。",
                format_nums(&expected),
                format_nums(&used)
            );
        }

        self.sessions.remove(key);
        format!(
            "恭喜你，回答正确！🎉\n题目数字：{}\n你的答案：{} = 24",
            format_nums(&session.nums),
            input
        )
    }

    pub fn give_up(&mut self, key: &GameSessionKey, now: Instant) -> String {
        let Some(session) = self.sessions.get(key).cloned() else {
            return no_game(&self.prefix);
        };
        if now >= session.expires_at {
            self.sessions.remove(key);
            return no_game(&self.prefix);
        }

        self.sessions.remove(key);
        format!(
            "好的，这一局就到这里。\n题目数字：{}\n一个可能的答案是：\n{} = 24",
            format_nums(&session.nums),
            session.solution
        )
    }

    pub fn expire(&mut self, now: Instant) -> Vec<TimeoutNotice> {
        let keys: Vec<GameSessionKey> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.expires_at <= now)
            .map(|(key, _)| key.clone())
            .collect();

        let mut notices = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(session) = self.sessions.remove(&key) {
                notices.push(TimeoutNotice {
                    address: session.address,
                    text: format!(
                        "24 点游戏时间到了（{} 秒）。\n题目数字：{}\n一个可能的答案是：\n{} = 24",
                        self.time_limit.as_secs(),
                        format_nums(&session.nums),
                        session.solution
                    ),
                });
            }
        }
        notices
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.sessions
            .values()
            .map(|session| session.expires_at)
            .min()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    #[cfg(test)]
    fn insert_for_test(&mut self, key: GameSessionKey, session: GameSession) {
        self.sessions.insert(key, session);
    }
}

fn no_game(prefix: &str) -> String {
    format!(
        "你当前没有进行中的 24 点游戏，或游戏已经超时。\n请先发送 {prefix}24 开始一局新的游戏。"
    )
}

fn format_nums(nums: &[i32]) -> String {
    nums.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::basic::random::SeededRandom;
    use lukosbot_domain::{ChatKind, PlatformChatId};

    fn key(platform: PlatformKind, user: &str) -> GameSessionKey {
        GameSessionKey {
            platform,
            user_id: PlatformUserId::new(user).unwrap(),
        }
    }

    fn address() -> ChatAddress {
        ChatAddress::new(
            PlatformKind::Telegram,
            PlatformChatId::new("1").unwrap(),
            ChatKind::Private,
        )
    }

    fn manager() -> GameManager {
        GameManager::new(Duration::from_secs(300), Arc::new(SeededRandom::new(9)))
    }

    #[test]
    fn prompts_use_the_configured_prefix() {
        let mut manager = GameManager::with_prefix(
            Duration::from_secs(300),
            "!",
            Arc::new(SeededRandom::new(10)),
        );
        let now = Instant::now();
        let text = manager.start(key(PlatformKind::Telegram, "1"), address(), now);
        assert!(text.contains("!24 <表达式>"));
        assert!(text.contains("!24 giveup"));
    }

    #[test]
    fn start_and_repeat_start_are_distinct() {
        let mut manager = manager();
        let now = Instant::now();
        let first = manager.start(key(PlatformKind::Telegram, "1"), address(), now);
        let repeat = manager.start(key(PlatformKind::Telegram, "1"), address(), now);
        assert!(first.contains("新的一局"));
        assert!(repeat.contains("已经有一局"));
        assert_eq!(manager.session_count(), 1);
    }

    #[test]
    fn correct_answer_removes_session() {
        let mut manager = manager();
        let now = Instant::now();
        let key = key(PlatformKind::Telegram, "1");
        let _ = manager.start(key.clone(), address(), now);
        let session = manager.sessions.get(&key).cloned().unwrap();
        let text = manager.submit(&key, &session.solution, now);
        assert!(text.contains("回答正确"));
        assert_eq!(manager.session_count(), 0);
    }

    #[test]
    fn wrong_number_multiset_is_reported() {
        let mut manager = manager();
        let now = Instant::now();
        let key = key(PlatformKind::Telegram, "1");
        manager.insert_for_test(
            key.clone(),
            GameSession {
                address: address(),
                nums: [1, 2, 3, 4],
                solution: "(1+2+3)*4".to_owned(),
                expires_at: now + Duration::from_secs(300),
            },
        );
        let text = manager.submit(&key, "6+6+6+6", now);
        assert!(text.contains("你的结果不是 24") || text.contains("数字与题目不一致"));
    }

    #[test]
    fn expiry_returns_original_address() {
        let mut manager = manager();
        let now = Instant::now();
        let key = key(PlatformKind::Telegram, "1");
        let _ = manager.start(key, address(), now);
        let notices = manager.expire(now + Duration::from_secs(301));
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].address, address());
        assert!(notices[0].text.contains("时间到了"));
    }

    #[test]
    fn platforms_do_not_share_sessions() {
        let mut manager = manager();
        let now = Instant::now();
        manager.start(key(PlatformKind::Telegram, "1"), address(), now);
        manager.start(key(PlatformKind::Discord, "1"), address(), now);
        assert_eq!(manager.session_count(), 2);
    }

    #[test]
    fn a_known_session_can_be_inserted_for_state_tests() {
        let mut manager = manager();
        let now = Instant::now();
        let key = key(PlatformKind::Telegram, "1");
        manager.insert_for_test(
            key.clone(),
            GameSession {
                address: address(),
                nums: [1, 2, 3, 4],
                solution: "(1+2+3)*4".to_owned(),
                expires_at: now + Duration::from_secs(10),
            },
        );
        assert!(manager.give_up(&key, now).contains("(1+2+3)*4 = 24"));
    }
}
