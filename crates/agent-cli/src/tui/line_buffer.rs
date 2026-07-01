/// 换行边界门控 — 只释放到最末一个 \n 为止的文本
/// 防止部分 markdown（如未闭合的 ``` ）被渲染
#[derive(Debug, Default, Clone)]
pub struct LineBuffer {
    pending: String,
}

impl LineBuffer {
    pub fn new() -> Self { Self::default() }
    
    /// 追加原始 delta
    pub fn push(&mut self, delta: &str) {
        if delta.is_empty() { return; }
        self.pending.push_str(delta);
    }
    
    /// 返回到最末 \n 为止的可提交文本（含 \n），剩余部分保留
    pub fn take_committable(&mut self) -> String {
        let Some(last_nl) = self.pending.rfind('\n') else {
            return String::new();
        };
        self.pending.drain(..=last_nl).collect()
    }
    
    /// 流结束时刷新剩余内容
    pub fn flush(&mut self) -> String {
        std::mem::take(&mut self.pending)
    }
    
    /// 是否有未提交文本
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
    
    /// 当前未提交文本的引用
    pub fn pending_text(&self) -> &str {
        &self.pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_push_and_take() {
        let mut lb = LineBuffer::new();
        lb.push("hello\nworld\n");
        let committed = lb.take_committable();
        assert_eq!(committed, "hello\nworld\n");
        assert_eq!(lb.pending_text(), "");
    }

    #[test]
    fn test_partial_line_held_back() {
        let mut lb = LineBuffer::new();
        lb.push("hello\nwor");
        let committed = lb.take_committable();
        assert_eq!(committed, "hello\n");
        assert_eq!(lb.pending_text(), "wor");
    }

    #[test]
    fn test_no_newline_returns_empty() {
        let mut lb = LineBuffer::new();
        lb.push("hello world");
        let committed = lb.take_committable();
        assert!(committed.is_empty());
        assert_eq!(lb.pending_text(), "hello world");
    }

    #[test]
    fn test_flush_drains_all() {
        let mut lb = LineBuffer::new();
        lb.push("partial");
        let remaining = lb.flush();
        assert_eq!(remaining, "partial");
        assert!(!lb.has_pending());
    }

    #[test]
    fn test_multiple_pushes_accumulate() {
        let mut lb = LineBuffer::new();
        lb.push("hel");
        lb.push("lo\nwor");
        lb.push("ld\n");
        let committed = lb.take_committable();
        assert_eq!(committed, "hello\nworld\n");
    }
}
