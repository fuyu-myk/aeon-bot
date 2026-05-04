/// A single playing card
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    /// e.g. "A", "2", ..., "10", "J", "Q", "K"
    pub rank: String,
    /// e.g. "Hearts", "Diamonds", "Clubs", "Spades"
    pub suit: String,
    /// Numerical value for calculating hand totals
    /// (e.g. 2–10 for number cards, 10 for face cards, 11 for Aces)
    pub value: i8,
}

impl Card {
    pub fn new(rank: &str, suit: &str) -> Self {
        let value = match rank {
            "A" => 11,
            "K" | "Q" | "J" => 10,
            _ => rank.parse().unwrap_or(0),
        };

        Card {
            rank: rank.to_string(),
            suit: suit.to_string(),
            value,
        }
    }

    pub fn display(&self) -> String {
        let suit = match self.suit.as_str() {
            "Hearts" => "♥",
            "Diamonds" => "♦",
            "Clubs" => "♣",
            "Spades" => "♠",
            _ => unreachable!(),
        };

        format!("{}{}", self.rank, suit)
    }
}

/// A standard deck of 52 playing cards, represented as a boolean array
/// where `true` means the card is still available
///
/// Card indices are as follows (row-major order):
/// 0–12: Hearts A-K
/// 13–25: Clubs A-K
/// 26–38: Diamonds A-K
/// 39–51: Spades A-K
pub type Deck = [bool; 52];

/// Initialize a new deck with all cards available
pub fn new_deck() -> Deck {
    [true; 52]
}

/// Draw a random available card from the deck, marking it as drawn
///
/// Returns `None` if the deck is empty
pub fn draw_card(deck: &mut Deck) -> Option<usize> {
    use rand::Rng;

    // Shuffle indices of available cards and pick the first one
    let available_indices: Vec<usize> = deck
        .iter()
        .enumerate()
        .filter_map(|(i, &available)| if available { Some(i) } else { None })
        .collect();

    if available_indices.is_empty() {
        None
    } else {
        let mut rng = rand::thread_rng();
        let idx = rng.gen_range(0..available_indices.len());
        let card_index = available_indices[idx];
        deck[card_index] = false; // Mark the card as drawn

        Some(card_index)
    }
}

pub fn match_card(card_index: usize) -> Card {
    let rank: String = match card_index % 13 {
        0 => "A".to_string(),
        10 => "J".to_string(),
        11 => "Q".to_string(),
        12 => "K".to_string(),
        n => (n + 1).to_string(),
    };

    let suit = match card_index / 13 {
        0 => "Hearts",
        1 => "Clubs",
        2 => "Diamonds",
        3 => "Spades",
        _ => unreachable!(),
    };

    Card::new(&rank, suit)
}
