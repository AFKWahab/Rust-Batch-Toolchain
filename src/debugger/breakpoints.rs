use std::collections::HashSet;

pub struct Breakpoints {
    points: HashSet<usize>,
}

impl Breakpoints {
    pub fn new() -> Self {
        Self {
            points: HashSet::new(),
        }
    }

    pub fn add(&mut self, logical_line: usize) {
        self.points.insert(logical_line);
        eprintln!("Breakpoint set at logical line {}", logical_line);
    }

    pub fn remove(&mut self, logical_line: usize) {
        self.points.remove(&logical_line);
        eprintln!("Breakpoint removed from logical line {}", logical_line);
    }

    pub fn contains(&self, logical_line: usize) -> bool {
        self.points.contains(&logical_line)
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.points.clear();
    }
}
