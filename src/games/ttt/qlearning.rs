use sqlx::{SqlitePool, Row};


/// Q-learning agent for tic-tac-toe
///
/// The Q-table is persisted in SQLite. The bot always plays as `O`.
/// States are 9-character strings (e.g. `"_XO___X__"`).
/// Actions are cell positions 0–8
pub struct QLearner {
    /// Learning rate – how much new information overrides old.
    pub alpha: f64,
    /// Discount factor – how much future reward is valued.
    pub gamma: f64,
}

impl QLearner {
    pub fn new() -> Self {
        QLearner { alpha: 0.3, gamma: 0.9 }
    }

    /// Epsilon decays from 0.9 (full exploration) to 0.05 (mostly exploitation)
    /// over ~1 000 games, giving early games high variability and later games
    /// near-optimal play
    pub fn epsilon(total_games: i64) -> f64 {
        const MIN: f64 = 0.05;
        const MAX: f64 = 0.9;
        const DECAY: f64 = 0.005;

        MIN + (MAX - MIN) * (-DECAY * total_games as f64).exp()
    }

    pub async fn get_q(&self, db: &SqlitePool, state: &str, action: usize) -> f64 {
        sqlx::query(
            "SELECT q_value FROM ttt_q_table WHERE state_key = ? AND action = ?",
        )
        .bind(state)
        .bind(action as i64)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .map(|row| row.get::<f64, _>("q_value"))
        .unwrap_or(0.0)
    }

    pub async fn set_q(&self, db: &SqlitePool, state: &str, action: usize, value: f64) {
        let _ = sqlx::query(
            "INSERT INTO ttt_q_table (state_key, action, q_value, visit_count)
             VALUES (?, ?, ?, 1)
             ON CONFLICT(state_key, action) DO UPDATE SET
               q_value = excluded.q_value,
               visit_count = visit_count + 1",
        )
        .bind(state)
        .bind(action as i64)
        .bind(value)
        .execute(db)
        .await;
    }

    /// Epsilon-greedy action selection from available positions
    pub async fn select_action(
        &self,
        db: &SqlitePool,
        state: &str,
        available: &[usize],
        total_games: i64,
    ) -> usize {
        let eps = Self::epsilon(total_games);

        // Generate random values in a tight block so ThreadRng is dropped
        // before any `.await` (ThreadRng is !Send – it cannot cross await points)
        let (explore, random_idx) = {
            use rand::Rng;

            let mut rng = rand::thread_rng();
            let f: f64 = rng.r#gen();
            let idx: usize = rng.r#gen_range(0..available.len());

            (f < eps, idx)
        }; // rng dropped here

        if explore {
            available[random_idx]
        } else {
            let mut best_action = available[0];
            let mut best_q = f64::NEG_INFINITY;

            for &action in available {
                let q = self.get_q(db, state, action).await;

                if q > best_q {
                    best_q = q;
                    best_action = action;
                }
            }

            best_action
        }
    }

    /// Temporal-difference (TD-0) update over the bot's move history
    ///
    /// Walks backward through `history` (pairs of state-before-move and action)
    /// propagating discounted reward, so intermediate positions learn to
    /// anticipate the final outcome
    pub async fn td_update(
        &self,
        db: &SqlitePool,
        history: &[(String, usize)],
        final_reward: f64,
    ) {
        if history.is_empty() {
            return;
        }

        let mut future_val = final_reward;

        for (state, action) in history.iter().rev() {
            let old_q = self.get_q(db, state, *action).await;
            let new_q = old_q + self.alpha * (future_val - old_q);

            self.set_q(db, state, *action, new_q).await;
            future_val = self.gamma * new_q;
        }
    }
}