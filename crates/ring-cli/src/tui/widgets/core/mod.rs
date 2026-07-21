//! 可复用 TUI 组件原语（React 式：state/props 分离 + render-prop 闭包 + 组合优于继承）。
//!
//! - [`scroll_list::ScrollList`]：泛型可滚动可选列表的**导航状态**（cursor/scroll），
//!   数据每帧以 `&[T]` 作为 props 传入，行渲染通过闭包（render-prop）定制。
//! - [`scroll_list::anchor_above`]：把浮层锚定到输入框上方（吸收各 modal 重复的 `area()`）。
//! - [`scroll_list::pointer`] / [`scroll_list::label`]：`❯` 焦点指针与选中高亮的统一样式。

pub mod scroll_list;
