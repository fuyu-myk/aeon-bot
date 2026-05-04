/// Tic-tac-toe cell state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cell {
    Empty,
    X,
    O,
}

#[allow(dead_code)]
impl Cell {
    pub fn to_char(self) -> char {
        match self {
            Cell::Empty => '_',
            Cell::X => 'X',
            Cell::O => 'O',
        }
    }

    pub fn from_char(c: char) -> Self {
        match c {
            'X' => Cell::X,
            'O' => Cell::O,
            _ => Cell::Empty,
        }
    }

    pub fn emoji(self) -> &'static str {
        match self {
            Cell::Empty => "⬜",
            Cell::X => "❌",
            Cell::O => "⭕",
        }
    }
}

/// Immutable 3×3 tic-tac-toe board (row-major positions 0–8)
///
/// ```
/// 0 | 1 | 2
/// 3 | 4 | 5
/// 6 | 7 | 8
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Board {
    cells: [Cell; 9],
}

const WIN_LINES: [[usize; 3]; 8] = [
    [0, 1, 2],
    [3, 4, 5],
    [6, 7, 8],
    [0, 3, 6],
    [1, 4, 7],
    [2, 5, 8],
    [0, 4, 8],
    [2, 4, 6],
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GameResult {
    Win(Cell),
    Draw,
    InProgress,
}

#[allow(dead_code)]
impl Board {
    pub fn new() -> Self {
        Board {
            cells: [Cell::Empty; 9],
        }
    }

    pub fn from_state(state: &str) -> Self {
        let mut cells = [Cell::Empty; 9];
        for (i, c) in state.chars().enumerate().take(9) {
            cells[i] = Cell::from_char(c);
        }

        Board { cells }
    }

    pub fn to_state_key(&self) -> String {
        self.cells.iter().map(|c| c.to_char()).collect()
    }

    /// Returns a new Board with the given cell set (immutable pattern)
    pub fn with_move(&self, pos: usize, cell: Cell) -> Self {
        let mut new_cells = self.cells;
        new_cells[pos] = cell;

        Board { cells: new_cells }
    }

    pub fn get(&self, pos: usize) -> Cell {
        self.cells[pos]
    }

    pub fn available_moves(&self) -> Vec<usize> {
        (0..9).filter(|&i| self.cells[i] == Cell::Empty).collect()
    }

    pub fn check_winner(&self) -> Option<Cell> {
        for &[a, b, c] in &WIN_LINES {
            if self.cells[a] != Cell::Empty
                && self.cells[a] == self.cells[b]
                && self.cells[b] == self.cells[c]
            {
                return Some(self.cells[a]);
            }
        }

        None
    }

    pub fn is_full(&self) -> bool {
        self.cells.iter().all(|&c| c != Cell::Empty)
    }

    pub fn result(&self) -> GameResult {
        match self.check_winner() {
            Some(winner) => GameResult::Win(winner),
            None if self.is_full() => GameResult::Draw,
            None => GameResult::InProgress,
        }
    }
}
