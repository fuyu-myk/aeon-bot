pub mod commands;
pub mod interactions;

use std::{collections::HashMap, sync::Arc, time::Instant};

use ::serenity::all::{ButtonStyle, CreateActionRow, CreateButton};
use poise::serenity_prelude as serenity;
use serenity::all::{ChannelId, GuildId, MessageId, UserId};
use tokio::sync::Mutex;

use crate::games::cards::{Card, Deck, draw_card, match_card, new_deck};

/// An active blackjack game session
#[derive(Clone, Debug)]
pub struct BjGame {
    pub game_id: String,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    /// The message displaying the game state, which players interact with via buttons
    pub message_id: MessageId,
    pub host_id: UserId,
    pub max_players: usize,
    /// (player_id, initial_wager) — live wager during a game lives in PlayerHand
    pub players: Vec<(UserId, i64)>,
    pub dealer_hand: Hand,
    pub deck: Deck,
    /// Whether dealer's hand is hidden or fully revealed
    pub dealer_hidden: bool,
    pub last_activity: Instant,
    pub current_phase: BjPhase,
    pub player_hands: Vec<PlayerHand>,
    /// Index of the currently playing hand
    pub current: usize,
}

impl BjGame {
    /// Create a singleplayer game, dealing cards immediately
    pub fn new(
        game_id: String,
        guild_id: GuildId,
        channel_id: ChannelId,
        message_id: MessageId,
        host_id: UserId,
        wager: i64,
    ) -> Self {
        let mut deck = new_deck();
        let dealer_hand = Self::draw_dealer_hand(&mut deck);
        let player_hands = vec![Self::draw_player_hand(&mut deck, wager)];

        let mut game = BjGame {
            game_id,
            guild_id,
            channel_id,
            message_id,
            host_id,
            max_players: 1,
            players: vec![(host_id, wager)],
            dealer_hand,
            deck,
            dealer_hidden: true,
            last_activity: Instant::now(),
            current_phase: BjPhase::PlayerTurn,
            player_hands,
            current: 0,
        };
        game.auto_advance_past_inactive();
        game
    }

    /// Create a multiplayer lobby; cards are not dealt until `start_game` is called
    pub fn new_lobby(
        game_id: String,
        guild_id: GuildId,
        channel_id: ChannelId,
        message_id: MessageId,
        host_id: UserId,
        wager: i64,
    ) -> Self {
        BjGame {
            game_id,
            guild_id,
            channel_id,
            message_id,
            host_id,
            max_players: 4,
            players: vec![(host_id, wager)],
            dealer_hand: Hand::empty(),
            deck: new_deck(),
            dealer_hidden: true,
            last_activity: Instant::now(),
            current_phase: BjPhase::Lobby,
            player_hands: vec![],
            current: 0,
        }
    }

    /// Deal cards and transition from Lobby → PlayerTurn
    ///
    /// Must be called exactly once when the host starts the game
    pub fn start_game(&mut self) {
        let mut deck = new_deck();
        self.dealer_hand = Self::draw_dealer_hand(&mut deck);
        self.player_hands = self
            .players
            .iter()
            .map(|(_, wager)| Self::draw_player_hand(&mut deck, *wager))
            .collect();
        self.deck = deck;
        self.last_activity = Instant::now();
        self.current_phase = BjPhase::PlayerTurn;
        self.auto_advance_past_inactive();
    }

    pub fn is_lobby_full(&self) -> bool {
        self.players.len() >= self.max_players
    }

    /// Calculate the total value of a hand, counting Aces as 1 or 11 as appropriate
    pub fn calculate_hand_total(hand: &[Card]) -> i64 {
        let mut total = 0;
        let mut aces = 0;

        for card in hand {
            total += card.value as i64;
            if card.rank == "A" {
                aces += 1;
            }
        }

        // Downgrade Aces from 11 to 1 as needed to avoid busting
        while total > 21 && aces > 0 {
            total -= 10;
            aces -= 1;
        }

        total
    }

    fn draw_starting_hand(deck: &mut Deck) -> Vec<Card> {
        let mut cards = Vec::new();

        for _ in 0..2 {
            if let Some(card_index) = draw_card(deck) {
                let card = match_card(card_index);
                cards.push(card);
            }
        }

        cards
    }

    fn draw_dealer_hand(deck: &mut Deck) -> Hand {
        let cards = Self::draw_starting_hand(deck);
        let total = Self::calculate_hand_total(&cards);

        Hand {
            cards,
            total: total as i8,
            is_busted: total > 21,
            is_blackjack: total == 21,
        }
    }

    fn draw_player_hand(deck: &mut Deck, initial_wager: i64) -> PlayerHand {
        let player_cards = Self::draw_starting_hand(deck);
        let total = Self::calculate_hand_total(&player_cards);
        let is_blackjack = total == 21;
        let hand = Hand {
            cards: player_cards,
            total: total as i8,
            is_busted: total > 21,
            is_blackjack,
        };

        PlayerHand {
            hand,
            wager: initial_wager,
            state: if is_blackjack {
                HandState::Blackjack
            } else {
                HandState::Active
            },
            is_split: false,
            resolution: None,
        }
    }

    pub fn render_message(&self) -> String {
        let mut message = String::new();

        let dealer_display = Self::render_dealer_hand(&self.dealer_hand, self.dealer_hidden);
        message.push_str(&format!("**Dealer's Hand:** {}\n", dealer_display));

        let players_display = Self::render_players_hands(self, &self.player_hands);
        message.push_str(&players_display);

        message
    }

    pub fn render_lobby(&self) -> String {
        let header = format!("**Blackjack Lobby** (host: <@{}>)\n", self.host_id);
        let player_lines: Vec<String> = self
            .players
            .iter()
            .enumerate()
            .map(|(i, (id, wager))| format!("{}. <@{id}> — Wager: {wager} pts", i + 1))
            .collect();
        let join_count = format!(
            "\n\n{}/{} players joined",
            self.players.len(),
            self.max_players
        );

        format!("{}{}{}", header, player_lines.join("\n"), join_count)
    }

    fn render_dealer_hand(dealer_hand: &Hand, hidden: bool) -> String {
        if hidden {
            if let Some(first_card) = dealer_hand.cards.first() {
                format!(
                    "{} and [Hidden] (showing: {})",
                    first_card.display(),
                    first_card.value
                )
            } else {
                "[No Cards]".to_string()
            }
        } else {
            let card_displays: Vec<String> =
                dealer_hand.cards.iter().map(|c| c.display()).collect();
            format!("{} [{}]", card_displays.join(", "), dealer_hand.total)
        }
    }

    fn render_players_hands(&self, player_hands: &[PlayerHand]) -> String {
        let mut display = String::new();

        for (i, player_hand) in player_hands.iter().enumerate() {
            let mention = self.get_username(i);
            let card_displays: Vec<String> =
                player_hand.hand.cards.iter().map(|c| c.display()).collect();
            let hand_display = card_displays.join(", ");
            let status = match player_hand.state {
                HandState::Active => {
                    if i == self.current {
                        format!("▶ <@{}>'s turn", self.players[self.current].0)
                    } else {
                        String::new()
                    }
                }
                HandState::Stood => "Stood".to_string(),
                HandState::Busted => "💥 Busted!".to_string(),
                HandState::Blackjack => "🃏 Blackjack!".to_string(),
            };
            let outcome = if player_hand.resolution.is_some() {
                self.render_resolution_text(player_hand)
            } else {
                &status
            };
            let wager = player_hand.wager;
            let split_label = if player_hand.is_split { " (split)" } else { "" };
            let total = player_hand.hand.total;

            display.push_str(&format!(
                "{mention}{split_label}: {hand_display} [{total}] {outcome} | Wager: {wager}\n"
            ));
        }

        display
    }

    fn get_username(&self, player_idx: usize) -> String {
        let (id, _) = self
            .players
            .get(player_idx)
            .expect("Player index should be valid");
        format!("<@{id}>")
    }

    fn render_resolution_text(&self, player_hand: &PlayerHand) -> &'static str {
        match player_hand.resolution.unwrap() {
            BjResolution::PlayerWin(win_type) => match win_type {
                BjWinType::Blackjack => "You win with a Blackjack! 🎉",
                BjWinType::DealerBust => "You win! Dealer busted. 🎉",
                BjWinType::Regular => "You win! 🎉",
            },
            BjResolution::DealerWin(loss_reason) => match loss_reason {
                BjLossReason::PlayerBust => "You lose! You busted. 💥",
                BjLossReason::DealerHigher => "You lose! Dealer had a higher hand. 😢",
                BjLossReason::DealerBlackjack => "You lose! Dealer had a Blackjack. 😢",
            },
            BjResolution::Surrender => "You surrendered. 😢",
            BjResolution::Push => "It's a push (tie). 🤝",
        }
    }

    fn find_next_active_player(&self) -> Option<usize> {
        let len = self.player_hands.len();
        self.player_hands[self.current + 1..len]
            .iter()
            .position(|h| h.state == HandState::Active)
    }

    fn advance_current_player(&mut self) {
        if let Some(next_player) = self.find_next_active_player() {
            self.current += next_player + 1;
        } else {
            self.current_phase = BjPhase::DealerTurn;
        }
    }

    fn hit(&self, deck: &mut Deck) -> Option<Card> {
        if let Some(card_index) = draw_card(deck) {
            let card = match_card(card_index);
            Some(card)
        } else {
            None
        }
    }

    fn draw_player_card(&mut self, deck: &mut Deck) {
        let new_card = match self.hit(deck) {
            Some(card) => card,
            None => {
                // TODO: maybe reshuffle
                self.current_phase = BjPhase::GameOver;
                return;
            }
        };

        let current_hand = &mut self.player_hands[self.current].hand;
        current_hand.cards.push(new_card);

        current_hand.total = Self::calculate_hand_total(&current_hand.cards) as i8;
        current_hand.is_busted = current_hand.total > 21;

        if current_hand.is_busted {
            self.player_hands[self.current].state = HandState::Busted;
        } else if current_hand.total == 21 {
            self.player_hands[self.current].state = HandState::Stood;
        }
    }

    fn clone_cards(&self, player_id: usize) -> Vec<Card> {
        self.player_hands[player_id].hand.cards.clone()
    }

    fn apply_hit(&mut self, deck: &mut Deck) {
        self.draw_player_card(deck);

        if self.player_hands[self.current].state == HandState::Active {
            return;
        }

        self.advance_current_player();
    }

    fn apply_stand(&mut self) {
        self.player_hands[self.current].state = HandState::Stood;
        self.advance_current_player();
    }

    fn apply_double_down(&mut self, deck: &mut Deck) {
        self.player_hands[self.current].wager *= 2;
        self.draw_player_card(deck);

        if !self.player_hands[self.current].hand.is_busted {
            self.player_hands[self.current].state = HandState::Stood;
        }

        self.advance_current_player();
    }

    fn apply_split(&mut self, deck: &mut Deck, wager: i64) {
        let mut hand_a_cards = self.clone_cards(self.current);
        let mut hand_b_cards = hand_a_cards.split_off(1);

        let hand_a_new_card = self
            .hit(deck)
            .expect("Deck should have enough cards for split");
        let hand_b_new_card = self
            .hit(deck)
            .expect("Deck should have enough cards for split");
        hand_a_cards.push(hand_a_new_card);
        hand_b_cards.push(hand_b_new_card);

        let hand_a = Hand::new(hand_a_cards);
        let hand_b = Hand::new(hand_b_cards);

        self.player_hands[self.current].hand = hand_a;
        self.player_hands[self.current].is_split = true;
        if self.player_hands[self.current].hand.is_blackjack {
            self.player_hands[self.current].state = HandState::Blackjack;
        }

        let mut new_player_hand = PlayerHand::new(hand_b, wager, true);
        if new_player_hand.hand.is_blackjack {
            new_player_hand.state = HandState::Blackjack;
        }

        self.players
            .insert(self.current + 1, self.players[self.current]);
        self.player_hands.insert(self.current + 1, new_player_hand);

        if self.player_hands[self.current].state != HandState::Active {
            self.advance_current_player();
        }
    }

    fn apply_surrender(&mut self) {
        self.player_hands[self.current].state = HandState::Stood;
        self.player_hands[self.current].resolution = Some(BjResolution::Surrender);
        self.advance_current_player();
    }

    /// Advance `current` past any non-Active hands at the start of PlayerTurn
    ///
    /// Called after dealing to skip players who have initial blackjacks
    pub fn auto_advance_past_inactive(&mut self) {
        while self.current_phase == BjPhase::PlayerTurn
            && self.player_hands[self.current].state != HandState::Active
        {
            self.advance_current_player();
        }
    }

    fn dealer_turn(&mut self, deck: &mut Deck) {
        self.dealer_hidden = false;

        while self.dealer_hand.total < 17 {
            if let Some(card_index) = draw_card(deck) {
                let card = match_card(card_index);
                self.dealer_hand.cards.push(card);

                self.dealer_hand.total = Self::calculate_hand_total(&self.dealer_hand.cards) as i8;
                self.dealer_hand.is_busted = self.dealer_hand.total > 21;
            } else {
                // TODO: reshuffle?
            }
        }
    }

    fn resolve_game(&mut self) -> Vec<(usize, BjResolution, i64)> {
        let mut results = Vec::new();

        let dealer_total = self.dealer_hand.total;
        let dealer_busted = self.dealer_hand.is_busted;
        let dealer_blackjack = self.dealer_hand.is_blackjack;

        for (i, player_hand) in self.player_hands.iter_mut().enumerate() {
            if player_hand.resolution == Some(BjResolution::Surrender) {
                let delta = -(player_hand.wager / 2);
                results.push((i, BjResolution::Surrender, delta));
                continue;
            }

            let player_total = player_hand.hand.total;
            let (resolution, delta) = if player_hand.hand.is_busted {
                (
                    BjResolution::DealerWin(BjLossReason::PlayerBust),
                    -player_hand.wager,
                )
            } else if player_hand.hand.is_blackjack && dealer_blackjack {
                (BjResolution::Push, 0)
            } else if player_hand.hand.is_blackjack {
                (
                    BjResolution::PlayerWin(BjWinType::Blackjack),
                    (player_hand.wager as f64 * 1.5) as i64,
                )
            } else if dealer_blackjack {
                (
                    BjResolution::DealerWin(BjLossReason::DealerBlackjack),
                    -player_hand.wager,
                )
            } else if dealer_busted {
                (
                    BjResolution::PlayerWin(BjWinType::DealerBust),
                    player_hand.wager,
                )
            } else if player_total > dealer_total {
                (
                    BjResolution::PlayerWin(BjWinType::Regular),
                    player_hand.wager,
                )
            } else if player_total == dealer_total {
                (BjResolution::Push, 0)
            } else {
                (
                    BjResolution::DealerWin(BjLossReason::DealerHigher),
                    -player_hand.wager,
                )
            };

            player_hand.resolution = Some(resolution);
            results.push((i, resolution, delta));
        }

        self.current_phase = BjPhase::GameOver;
        results
    }
}

pub type BjGames = Arc<Mutex<HashMap<String, BjGame>>>;

#[derive(Clone, Debug)]
pub struct PlayerHand {
    pub hand: Hand,
    pub wager: i64,
    pub state: HandState,
    pub is_split: bool,
    pub resolution: Option<BjResolution>,
}

impl PlayerHand {
    fn new(hand: Hand, wager: i64, is_split: bool) -> Self {
        PlayerHand {
            hand,
            wager,
            state: HandState::Active,
            is_split,
            resolution: None,
        }
    }
}

/// A generic hand of cards
#[derive(Clone, Debug)]
pub struct Hand {
    pub cards: Vec<Card>,
    pub total: i8,
    pub is_busted: bool,
    pub is_blackjack: bool,
}

impl Hand {
    fn new(cards: Vec<Card>) -> Self {
        let total = BjGame::calculate_hand_total(&cards) as i8;

        Hand {
            cards,
            total,
            is_busted: total > 21,
            is_blackjack: total == 21,
        }
    }

    fn empty() -> Self {
        Hand {
            cards: vec![],
            total: 0,
            is_busted: false,
            is_blackjack: false,
        }
    }
}

/// The current state of a player's hand,
/// which determines which actions are valid
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandState {
    Active,
    Stood,
    Busted,
    Blackjack,
}

/// Phase of a blackjack game, which determines what actions are valid
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BjPhase {
    Lobby,
    PlayerTurn,
    DealerTurn,
    GameOver,
}

/// Final outcome of a blackjack game
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BjResolution {
    PlayerWin(BjWinType),
    DealerWin(BjLossReason),
    Surrender,
    /// Essentially a tie
    Push,
}

/// Specific win types for the player, used to calculate payouts
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BjWinType {
    /// Player gets 21 on the initial deal (Ace + 10-value card)
    Blackjack,
    /// Player wins by having a higher total than the dealer without busting
    Regular,
    /// Player wins because the dealer busts by exceeding 21
    DealerBust,
}

/// Specific loss reasons for the player, used for game outcome messaging
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BjLossReason {
    /// Player exceeds 21 and busts
    PlayerBust,
    /// Dealer has a higher total than the player without busting
    DealerHigher,
    /// Dealer gets 21 on the initial deal (Ace + 10-value card)
    DealerBlackjack,
}

// ── Action row rendering ──────────────────────────────────────────────────────

pub fn make_action_buttons(game_id: &str, action_set: ActionSet) -> CreateActionRow {
    let buttons = vec![
        CreateButton::new(format!("bj_hit_{}", game_id))
            .label("Hit")
            .disabled(!action_set.can_hit),
        CreateButton::new(format!("bj_stand_{}", game_id))
            .label("Stand")
            .disabled(!action_set.can_stand),
        CreateButton::new(format!("bj_double_{}", game_id))
            .label("Double Down")
            .disabled(!action_set.can_double),
        CreateButton::new(format!("bj_split_{}", game_id))
            .label("Split")
            .disabled(!action_set.can_split),
        CreateButton::new(format!("bj_surrender_{}", game_id))
            .label("Surrender")
            .style(ButtonStyle::Danger)
            .disabled(!action_set.can_surrender),
    ];

    CreateActionRow::Buttons(buttons)
}

pub fn make_lobby_buttons(lobby_id: &str, host_acting: bool, can_start: bool) -> CreateActionRow {
    let buttons = vec![
        CreateButton::new(format!("bj_lobby_join_{}", lobby_id))
            .label("Join")
            .disabled(host_acting),
        CreateButton::new(format!("bj_lobby_start_{}", lobby_id))
            .label("Start")
            .disabled(!can_start),
        CreateButton::new(format!("bj_lobby_leave_{}", lobby_id))
            .label("Leave")
            .style(ButtonStyle::Danger),
    ];

    CreateActionRow::Buttons(buttons)
}

// ── Game logic helpers ───────────────────────────────────────────────────────

/// Set of boolean flags indicating which actions are
/// currently valid for the player
pub struct ActionSet {
    pub can_hit: bool,
    pub can_stand: bool,
    pub can_double: bool,
    pub can_split: bool,
    pub can_surrender: bool,
}

pub fn actionset_disabled() -> ActionSet {
    ActionSet {
        can_hit: false,
        can_stand: false,
        can_double: false,
        can_split: false,
        can_surrender: false,
    }
}

/// Compute the set of valid actions for the player
/// based on the current game state and their balance
pub fn compute_available_actions(game: &BjGame, balance: i64) -> ActionSet {
    let can_hit = can_player_move(game);
    let can_stand = can_player_move(game);
    let can_double = can_double_down(game, balance);
    let can_split = can_split_hand(game, balance);
    let can_surrender = can_surrender(game);

    ActionSet {
        can_hit,
        can_stand,
        can_double,
        can_split,
        can_surrender,
    }
}

fn can_player_move(game: &BjGame) -> bool {
    game.current_phase == BjPhase::PlayerTurn
        && game.player_hands[game.current].state == HandState::Active
}

fn can_double_down(game: &BjGame, balance: i64) -> bool {
    can_player_move(game)
        && game.player_hands[game.current].hand.cards.len() == 2
        && balance >= game.player_hands[game.current].wager
}

fn can_split_hand(game: &BjGame, balance: i64) -> bool {
    if !can_player_move(game) {
        return false;
    }
    let hand = &game.player_hands[game.current].hand;
    hand.cards.len() == 2
        && balance >= game.player_hands[game.current].wager
        && !game.player_hands[game.current].is_split
        && hand.cards[0].value == hand.cards[1].value
}

fn can_surrender(game: &BjGame) -> bool {
    can_player_move(game)
        && game.player_hands[game.current].hand.cards.len() == 2
        && !game.player_hands[game.current].is_split
}
