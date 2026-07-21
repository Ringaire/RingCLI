//! ring-linkkit: Linkkit 协议运行时层。
//!
//! 在 [`linkkit_parser`]（XML 标签解析）之上实现协议语义：
//! - **两道闸**：read-gate（edit 前必须先 read）、doc-gate（tool-use 前必须先 doc-read）
//! - **七态状态机**：Idle / Reasoning / Executing / Gated / AwaitingUser / Suspended / Recovering
//! - **事件订阅**：`<event>` await（bash/agent/pid）与 autonomous（time/file/cond）
//!
//! 当前为阶段 0 占位 crate，具体实现设计见
//! `.agent/Neko-all/ringcli/linkkit-implementation-design.md`。
//!
//! 依赖方向：`ring-linkkit` → `linkkit-parser`（仅解析层，零 IO）。
//! 上层 `ring-engine` 负责把解析结果接入 agent executor。

pub use linkkit_parser as parser;
