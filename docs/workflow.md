# DeepX Reactor Fork 工作流

> 本仓库（`deepx-winui-reactor`，`microsoft/windows-rs` 的 fork）的协作纪律。
> DeepX 桌面端依赖本仓库 `deepx-winui` 分支（本地 path 引用，release 打包前切 git rev）。

## 拓扑

```
master（官方镜像，只读）
  └── deepx-winui（补丁栈集线，唯一长期分支）
        └── fx/<功能名>（功能验证分支，短命）
              ├── 开发 → 三层验证
              └── squash merge → deepx-winui → push origin → 删分支
```

## 规则

1. **master 只读**：禁止直接 commit/push 改动。只允许跟随官方：
   ```bash
   git fetch upstream master --depth 1
   git push origin master            # fast-forward 同步镜像
   ```
   （rebase 基线等特殊操作由维护者执行，可 force push。）

2. **一切修改从 deepx-winui 切分支**：
   ```bash
   git switch deepx-winui && git switch -c fx/<功能名>
   ```

3. **merge 方式：squash**：功能分支的实验提交压成 1 个原子补丁进
   deepx-winui——补丁栈的每个提交都应"有意义且可重放"（rebase 上游时
   冲突最少的关键）。

4. **三层验证门槛**（合入前必须全过）：
   ```bash
   cargo check -p windows-reactor        # fork 零警告
   cargo test -p windows-reactor         # fork 测试
   cargo check -p deepx-winui            # DeepX（path 依赖）零警告
   ```

5. **合入后删分支**：`git branch -d fx/<功能名>`，fork 上不留功能分支
   （远端只保留 master + deepx-winui 两条线）。

6. **上游更新路径唯一**：先更新只读 `master`，再重放 `deepx-winui`：
   ```bash
   git switch master
   git pull --ff-only upstream master
   git switch deepx-winui
   git rebase master
   # 或只想试某个上游 commit：
   git cherry-pick -n <sha>
   ```
   master 分支不动。

7. **提交信息**：`类型(范围): 中文描述`（feat/fix/chore/docs/test），
   正文说明动机与验证结果。

## 命令速查

```bash
# 查看我们对上游的全部净改动（审计总账）
git diff master..deepx-winui

# 导出补丁（PR / 留档）
git format-patch master..deepx-winui

# 上游更新的标准流程
git switch master
git pull --ff-only upstream master
git switch deepx-winui
git rebase master
# 解冲突 → 三层验证 → push origin deepx-winui
```

## 背景

- 当前仓库是完整 clone；旧 shallow fork `F:\deepx-winui-reactor-1` 只作为
  迁移来源，不再参与同步。
- 2026-08-11 已将补丁重放到上游 `c318f55a2`（#4824）。
- Element 便捷修饰方法（`modifiers_mut` / margin / transition /
  grid_column / automation_* / keyboard_accelerator / on_pointer_pressed
  等）为 DeepX 专属 extension（上游 capability 模型不为 Element 提供
  builder，DeepX 既有"函数返回 Element 后再链式修饰"模式依赖）。
