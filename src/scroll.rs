//! Scroll position of a list that can stick to its end (`tail -f`), shared
//! by the log windows. Pure, like [`crate::app`].

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scroll {
    /// Index of the top visible line when not following.
    top: usize,
    /// Stick to the end.
    follow: bool,
}

impl Default for Scroll {
    fn default() -> Self {
        Self {
            top: 0,
            follow: true,
        }
    }
}

impl Scroll {
    /// The top visible line of `len` lines in a viewport of `height`.
    pub fn top(&self, len: usize, height: usize) -> usize {
        let max = len.saturating_sub(height);
        if self.follow { max } else { self.top.min(max) }
    }

    pub fn is_following(&self) -> bool {
        self.follow
    }

    /// Scrolls by `delta` lines; reaching the end turns following back on.
    pub fn scroll_by(&mut self, delta: isize, len: usize, height: usize) {
        let max = len.saturating_sub(height);
        let top = self.top(len, height).saturating_add_signed(delta).min(max);
        self.top = top;
        self.follow = top == max;
    }

    /// Stops following and puts line `index` at the top.
    pub fn jump(&mut self, index: usize) {
        self.top = index;
        self.follow = false;
    }

    pub fn follow(&mut self) {
        self.follow = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_end_until_scrolled_up() {
        let mut s = Scroll::default();
        assert_eq!(s.top(30, 10), 20);
        s.scroll_by(-5, 30, 10);
        assert_eq!(s.top(30, 10), 15);
        assert!(!s.is_following());
        // More lines do not move a scrolled view.
        assert_eq!(s.top(50, 10), 15);
        s.scroll_by(isize::MAX, 50, 10);
        assert!(s.is_following());
        assert_eq!(s.top(60, 10), 50);
        s.scroll_by(isize::MIN, 60, 10);
        assert_eq!(s.top(60, 10), 0);
        // Fewer lines than the viewport: always at the top.
        assert_eq!(Scroll::default().top(3, 10), 0);
    }

    #[test]
    fn jump_stops_following() {
        let mut s = Scroll::default();
        s.jump(4);
        assert!(!s.is_following());
        assert_eq!(s.top(30, 10), 4);
        s.follow();
        assert_eq!(s.top(30, 10), 20);
    }
}
