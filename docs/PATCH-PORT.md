# DeepX 补丁移植记录

> 状态：旧 fork 补丁已重放并完成与上游 `c318f55a2`（#4824）的语义融合。

## 基线

| 项 | 值 |
|---|---|
| 迁移来源 | `F:\deepx-winui-reactor-1`，`deepx-reactor`，`de10e3642` |
| 旧上游基线 | `9e4eb04e4`（#4807） |
| 新仓库 | `F:\deepx-winui-reactor` |
| 新上游基线 | `c318f55a254a4ceeca4e7a376bd22247201d83d8`（#4824） |
| 下游分支 | `deepx-winui` |
| 快照重放 | `42e951803` |

旧仓库是 shallow/incomplete clone；迁移完成后不再从该目录生成或更新补丁。

## 本轮上游变化

`5d7a5d889..c318f55a2` 共 10 个提交（#4815–#4824），集中于：

- 子投影与 mounted ownership 集中化；
- widget、结构更新、集合更新、destroy、root teardown 的失败恢复；
- templated list mount rollback；
- 删除不具备可靠失败所有权的 `CustomElement`；
- 未捕获 reconcile panic 后拒绝继续 reconcile。

## 冲突解决

### `backend/winui/mod.rs`

保留 #4815 删除 `parent_children` mirror 的实现。DeepX 的 RichText、templated
scroll、gradient 与 flyout 状态继续作为独立 backend 状态存在。

### `reconciler/templated.rs`

DeepX scroll wiring 已放入上游 `configure_templated_list`，因此配置、请求准备、
item source 更新与首次 apply 都受 #4822 的 `catch_unwind`/rollback 边界保护。
drain pass 使用上游 `assert_consistent_inner()`，不再调用旧断言。

### `reconciler/mod.rs` / `element.rs`

gradient prop diff 与 Element 便捷修饰保留；同时接受 #4823 删除
`CustomElement` 和 #4824 新增失败状态机。

### `tests/libs/reactor/src/lib.rs`

以最新上游 `RecordingBackend` 的 live-control、header/pane ownership 和 fault
injection 为准，只叠加 DeepX scroll recording/top handler。三种 scroll backend
操作均加入 templated mount failure matrix。

### Tab header element

旧快照包含公开 API 和 WinUI backend 分支，但缺少新版 reconciler 接线。本轮补齐
mount/update/unmount ownership，并验证 rich header 清除后恢复文本 fallback。

## 后续同步流程

```powershell
git switch master
git pull --ff-only upstream master
git switch deepx-winui
git rebase master

cargo check -p windows-reactor
cargo test -p test_reactor
cargo test -p test_reactor_selftest
git diff --check
```

每次同步后更新 `DEEPX-DOWNSTREAM.md` 的上游基线与冲突记录。
