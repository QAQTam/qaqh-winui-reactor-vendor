# DEEPX-DOWNSTREAM — reactor fork 补丁登记

本仓库是 `microsoft/windows-rs` 的 DeepX durable fork。`F:\DeepX` 在开发态以
`path` 依赖引用本仓库，发布态使用本仓库的 immutable git revision。

## 当前基线

| 项 | 值 |
|---|---|
| 上游 | `microsoft/windows-rs` `master` |
| 上游基线 | `c318f55a254a4ceeca4e7a376bd22247201d83d8`（#4824，2026-08-11） |
| 下游分支 | `deepx-winui` |
| 补丁快照 | `42e951803`（旧 fork 的净差异重放到新基线） |
| 消费端 | `F:\DeepX`，开发态路径 `F:\deepx-winui-reactor` |

旧 shallow fork `F:\deepx-winui-reactor-1` 仅作为迁移来源，不再作为构建、发布或
补丁审计依据。审计下游净差异使用：

```powershell
git diff master..deepx-winui
```

## 补丁登记表

| 组 | 能力 | 主要文件 |
|---|---|---|
| 引擎/诊断 | `DEEPX_PERF_LOG`、`set_render_observer`、`on_frame` | `engine.rs`、`hooks.rs`、`host.rs`、`lib.rs` |
| 虚拟列表 | follow/force tail、锚点保持、offset 恢复、顶部阈值与 viewport 回调 | `widget.rs`、`reconciler/templated.rs`、`backend/*` |
| RichText | 段落/run 增量 diff、run 样式、line height、text alignment | `widget.rs`、`widgets/text_block.rs`、`backend/winui/mod.rs`、bindings |
| 修饰系统 | `Element` 链式修饰、渐变前景、translation/transition | `element.rs`、`style.rs`、`reconciler/mod.rs` |
| WinUI 扩展 | rich flyout、Tab header element、图像与动画扩展 | `widgets/*`、`backend/winui/mod.rs` |
| ScrollViewer 滚动命令 | `scroll_to_bottom(generation)` 声明式滚动到底（ChangeView）；思考链路流式追底用 | `widgets/scroll_viewer.rs`、`generated.rs`、`backend/mod.rs`、`backend/winui/generated_set_prop.rs` |
| 生成绑定 | DeepX 使用的 WinUI 投影与 selftest bindings | `bindings.rs`、`reactor_selftest/src/bindings.rs`、`tools/reactor/src/base.txt` |

## c318f55a2 融合记录

- 保留 #4815 的 `MountedTree` 子投影所有权；未恢复已删除的 WinUI
  `parent_children` 镜像。
- DeepX templated scroll 配置位于 #4822 的 mount rollback 事务内；
  `configure`、`prepare`、`apply` 均纳入 fault injection 测试。
- 保留 #4824 的 reconcile 失败后 teardown-only 状态机。
- 接受 #4823 对 `CustomElement` 的删除；DeepX 消费端没有使用该 API。
- `RecordingBackend` 保留上游 live-control/ownership 一致性模型，并将 DeepX
  top-edge handler 纳入销毁与一致性检查。
- `TabItem::header_element` 已接入新版 mounted header ownership；移除 rich header
  时会先清空元素 header，再恢复文本 fallback。

## 2.0 等待期决策（2026-08-15）

- 决策：不追 1.x master（已冻结）、不提前迁 2.0（仍在 phase 2 review）、
  不在 1.x 上新增补丁；守住当前基线（`c318f55a`）等待 `reactor2` 定型。
- 发令枪信号（任一出现即重新评估）：
  1. `reactor2-phase2-review` 合回 master；
  2. #4835（Windows App SDK 2.4.0 适配）合并；
  3. windows-rs 下一个 release（74）。
- 2.0 验收清单（迁移前对照本表逐项验证）：
  1. VirtualList/VirtualGrid 是否覆盖 follow/force tail、锚点保持、
     offset 恢复、顶部阈值与 viewport 回调；
  2. 文本是否有增量段落模型（RichText 流式 delta 不整体重建）；
  3. `performance` API 是否覆盖 `set_render_observer`/`on_frame` 等价能力；
  4. 修饰系统：渐变前景、translation/transition；
  5. DeepX 在用控件覆盖：ContentDialog/TeachingTip/MenuFlyout/Tab 布局。
- 若 RichText 增量或聊天滚动语义缺失 → 暂缓迁移，或在 2.0 上重写对应补丁。

## 门禁

```powershell
cargo check -p windows-reactor
cargo test -p test_reactor
cargo test -p test_reactor_selftest
git diff --check
```

DeepX 侧还需运行：

```powershell
cargo check -p deepx-winui
cargo tree -p deepx-winui -d
```
