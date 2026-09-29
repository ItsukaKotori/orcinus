//! ConPTY 光标位置查询（CPR，Cursor Position Report）应答器。
//!
//! 终端侧会以 `ESC[6n` 查询光标位置；宿主若不应答 `ESC[<row>;<col>R`，依赖该应答的
//! 程序（如行编辑库）会静默卡住。[`scan_and_reply`] 在 PTY 输出字节流上扫描该查询并
//! 代为应答，同时维护跨块扫描所需的尾部状态。Task 5 的 reader 循环每读到一块输出即
//! 调用本函数；纯逻辑、平台无关，unix CI 可直接单测。

use std::io::Write;

const CPR_QUERY: &[u8] = b"\x1b[6n";
const CPR_REPLY: &[u8] = b"\x1b[1;1R";

/// 在 `tail`（上一块遗留的跨块扫描状态）与 `chunk` 拼接出的缓冲里扫描 `ESC[6n`，
/// 每命中一次向 `writer` 回写一次 `ESC[1;1R`（有命中才 flush），返回命中次数。
///
/// 尾部规则（修正 spike「命中即 `clear()`」丢同块后续半条查询的局限）：保留最后一次
/// 命中之后残余的**末尾 ≤3 字节**。能被下一块补全的半条查询必然是 4 字节查询的前缀
/// 且贴着流末尾，故至多 3 字节；其余残余不含可续接的 `ESC` 前缀，留与不留都不产生
/// 假命中，沿用 spike 的「末尾 ≤3 字节」简化规则（brief Step 2 认可，行为以测试为准）。
///
/// 写失败时返回 `Err`，此时 `tail` 已被置空、缓冲丢弃；调用方应把写失败视为会话
/// 致命错误，本 crate 内不存在携 `tail` 重试的路径。
pub fn scan_and_reply(
    tail: &mut Vec<u8>,
    chunk: &[u8],
    writer: &mut dyn Write,
) -> std::io::Result<usize> {
    let mut buf = std::mem::take(tail);
    buf.extend_from_slice(chunk);

    let mut hits = 0usize;
    let mut scanned = 0usize; // 已消费前缀长度 = 下一次搜索起点 = 最后一次命中的结束位置
    while let Some(offset) = buf[scanned..]
        .windows(CPR_QUERY.len())
        .position(|w| w == CPR_QUERY)
    {
        writer.write_all(CPR_REPLY)?;
        hits += 1;
        scanned += offset + CPR_QUERY.len();
    }
    if hits > 0 {
        writer.flush()?;
    }

    let keep = (buf.len() - scanned).min(CPR_QUERY.len() - 1);
    let split = buf.len() - keep;
    // 复用容量：整体移交后 drain 掉已消费前缀，避免每块重新分配（行为等价）。
    *tail = buf;
    tail.drain(..split);
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::{scan_and_reply, CPR_REPLY};

    #[derive(Default)]
    struct Sink(Vec<u8>);

    impl std::io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// spike 用例平移：查询跨块到达（`\x1b[6` + `n`），补全的那一块才回写一次。
    #[test]
    fn replies_when_query_splits_across_chunks() {
        let mut tail = Vec::new();
        let mut sink = Sink::default();
        let hits = scan_and_reply(&mut tail, b"\x1b[6", &mut sink).unwrap();
        assert_eq!(hits, 0);
        assert!(sink.0.is_empty());
        // 无命中：保留末尾 ≤3 字节；buf 恰为 3 字节的部分前缀 `\x1b[6`，整体留在
        // tail 等下一块补全。
        assert_eq!(tail, b"\x1b[6");
        let hits = scan_and_reply(&mut tail, b"n", &mut sink).unwrap();
        assert_eq!(hits, 1);
        assert_eq!(sink.0, CPR_REPLY);
        // 命中消费掉整个 buf，其后无残余 → tail 归空。
        assert!(tail.is_empty());
    }

    /// spike 用例平移：普通输出 + 半条查询，不回写，只保留末尾 ≤3 字节。
    #[test]
    fn keeps_only_partial_query_tail_without_replying() {
        let mut tail = Vec::new();
        let mut sink = Sink::default();
        let hits = scan_and_reply(&mut tail, b"output\x1b[", &mut sink).unwrap();
        assert_eq!(hits, 0);
        assert!(sink.0.is_empty());
        // CPR 查询是 4 字节；保留末尾至多 3 字节以覆盖分块边界。
        assert_eq!(tail, b"t\x1b[");
    }

    /// 修正 spike 局限的用例：命中之后同块（含后续块）的第二条查询不被丢失。
    ///
    /// spike 的 `reply_to_cursor_query` 命中一次就 `scan_tail.clear()` 且每次调用至多
    /// 回写一次：同块双查询只应答第一条，紧随其后的半条查询也被一并清掉。本用例的
    /// 三步分别在注释里推导中间态（brief ③ 要求）。
    #[test]
    fn replies_to_both_queries_in_same_chunk_and_keeps_partial_tail() {
        let mut tail = Vec::new();
        let mut sink = Sink::default();

        // 第一步：同一块内两条完整查询 `\x1b[6nABC\x1b[6n`（11 字节）。windows(4)
        // 非重叠命中 [0..4) 与 [7..11) → 回写两次（spike 只回写第一次）。第二次命中
        // 结束于 buf 末尾，其后残余为空 → tail 归空。brief 提到的「tail 非空、为
        // `\x1b[6n` 前缀、可被下一块补全」按算法只在命中之后仍有残余字节时出现，
        // 即第二步的场景；查询贴着流末尾时无可保留者。
        let hits = scan_and_reply(&mut tail, b"\x1b[6nABC\x1b[6n", &mut sink).unwrap();
        assert_eq!(hits, 2);
        assert_eq!(sink.0, [CPR_REPLY, CPR_REPLY].concat());
        assert!(tail.is_empty());

        // 第二步：命中后同块还跟着半条查询 `\x1b[6nABC\x1b[6`（10 字节）。命中
        // [0..4) 一次；其后残余 `ABC\x1b[6`（6 字节）取末 3 字节 → tail == b"\x1b[6"，
        // 非空、是 `\x1b[6n` 的前缀、可被下一块补全。spike 在此 clear() 会把它丢掉。
        let hits = scan_and_reply(&mut tail, b"\x1b[6nABC\x1b[6", &mut sink).unwrap();
        assert_eq!(hits, 1);
        assert_eq!(tail, b"\x1b[6");

        // 第三步：下一块 `n` 补全第二条查询（tail + chunk = `\x1b[6n`）→ 第三次回写；
        // 命中消费整条 buf，tail 归空。全程共 4 次回写。
        let hits = scan_and_reply(&mut tail, b"n", &mut sink).unwrap();
        assert_eq!(hits, 1);
        assert!(tail.is_empty());
        assert_eq!(
            sink.0,
            [CPR_REPLY, CPR_REPLY, CPR_REPLY, CPR_REPLY].concat()
        );
    }
}
