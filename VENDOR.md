# qaqh-winui-reactor-vendor

**Vendor snapshot（源码裁剪快照）— 冻结，不跟踪上游。**

本仓库是 `windows-reactor` 下游 fork 的**工作树快照**：已删除全部 git 历史，只保留构建所需源码。

## 来源声明

| 项 | 值 |
|---|---|
| 上游 | `microsoft/windows-rs` |
| 下游 fork | `QAQTam/qaq-winui-reactor`（分支 `deepx-winui`） |
| 冻结 commit | `74a6a4e5d`（最后一次 sync：Windows App SDK 2.4.0 #4835） |
| 冻结日期 | 2026-08-16 |
| 裁剪方式 | `git archive 74a6a4e5d`（无 .git 历史） |
| 保留的下游补丁 | ScrollViewer.scroll_to_bottom 滚动命令组（QAQ-Harness 思考链路追底依赖） |
| 当前 SDK pin | **2.5.1**（2026-09-21 由上游 `59cccbe70` 移植，见下节） |

## 冻结策略（2026-08-29 修订：自有组件化）

**上游 `microsoft/windows-rs` 已重写 reactor**，原「fork 分支 merge → 重新 archive」同步链在 fork 层即断裂，
重新同步等同整体移植。经评估**终止同步、本仓库转为自有组件直接维护**（冻结 tag：`frozen/2026-08-29`）：

- 消费方（qaqh-winui-app）通过 path 依赖指向本仓库工作树。
- 功能修改直接落在本仓库（ practice 自 2026-08 下旬已然如此，本条为追认），每次修改更新下方补丁清单。
- 重新评估迁移的唯一条件：重写后的上游给出我们**必须**的能力（当前没有）；届时按「逐补丁移植」立项，不做整体重写。
- ~~本仓库不接收功能修改~~（原策略，已废）。

## 补丁清单（vs 上游快照 `1ee42c9`，403 行 / 11 文件）

> 下表为冻结基线上的下游补丁；上游 SDK 版本推进（2.4.0 → 2.5.1）另见上节。

| 主题 | 文件 | 说明 |
|---|---|---|
| F-N11 布局隐式动画 | `composition/src/{animation,bindings,bindings_lifted,compositor}.rs`、`reactor/src/backend/winui/mod.rs` | Vector2 关键帧全链 + apply_layout_animation；F-N15 §5 关键锚点 |
| F-N15 STALE 防御 | `reactor/src/reconciler/mod.rs` | pass 后仍脏节点消费脏标记（A 方案），替代 debug_assert 中止 |
| NumberBox 压缩阈值旋钮 | `reactor/src/widgets/number_box.rs`、`backend/winui/{generated_set_prop.rs,bindings.rs}` | SmallChange/LargeChange/SpinButtonPlacementMode（**regen 时需保留**） |
| ScrollViewer 追底 | 快照自带 | scroll_to_bottom 滚动命令组（思考链路追底依赖） |
| B1 Slider 刻度三件套（2026-08-29） | `reactor/src/{bindings,generated}.rs`、`widgets/slider.rs`、`backend/{mod.rs,winui/mod.rs}`、`lib.rs` re-exports | `tick_frequency/tick_placement/snaps_to` 属性面；FFI 槽位补类型化 setter + SnapsTo/TickPlacement 枚举；回归 `tests/slider_ticks.rs` |
| B2 ThemeShadow/Elevation（2026-08-29） | `reactor/src/{bindings,style,element}.rs`、`reconciler/mod.rs`、`backend/{mod.rs,winui/mod.rs}` | `Modifiers.elevation` → ThemeShadow；receiver=直接父元素（insert_child 时解析——prop 阶段父未挂）；IThemeShadow.Receivers（UIElementWeakCollection 未投影，经 IVector<UIElement> 收货）+ IUIElement SetShadow/SetTranslation 透传；全 app 仅 composer 卡使用 |

待办补丁（冻结基线上实施）：
AnimatedIcon+AnimatedVisuals 通道（~150-250 行）· F-R3 删除线 / F-R4 Hyperlink · F-T1 TabView selection key。

## SDK 版本推进：2.4.0 → 2.5.1（2026-09-21）

上游 `59cccbe70`（`windows-reactor` update to Windows App SDK 2.5.1, #4951）已按「逐补丁移植」方式摘取到本仓库，
**不做整体重写**。该提交是纯版本号推进，**无 API 变化**。

| 文件 | 改动 |
|---|---|
| `crates/libs/reactor-setup/src/lib.rs` | `RUNTIME_VER` `2.4.0` → `2.5.1` |
| `crates/libs/reactor-setup/assets/app.manifest` | 三组件版本注释（Foundation 2.3.9→2.3.12、InteractiveExperiences 2.1.6→2.1.9、WinUI 2.3.6→2.3.9） |
| `crates/libs/reactor/src/bindings.rs` | `WINDOWSAPPSDK_RUNTIME_VERSION_UINT64` → `562971428323328` |
| `crates/tests/libs/reactor_selftest/src/bindings.rs` | 同上 |
| `crates/tools/reactor/src/main.rs` | `WINDOWS_APP_SDK_VERSION` → `2.5.1` |
| `crates/tools/reactor/src/extras.rdl` | 4 个常量 + 来源注释 |
| `.github/workflows/reactor.yml` | 安装器 URL → `windowsappsdk/2.5/2.5.1/` |

**未同步项**：`crates/tools/reactor/winmd/` 下 30 个 `.winmd` 二进制——本快照裁剪时已删除该目录，
无 regen 需求。上游该提交 33 个文件中只有 17 行文本对本仓库有效。

**`WINDOWSAPPSDK_RELEASE_MAJORMINOR` 故意保持 `131076` 不变**：上游新架构（`TryCreatePackageDependency`）
已删除该常量，而本 fork 仍走 `MddBootstrapInitialize2` 旧路径；该参数是**兼容族号**而非精确版本，
精确匹配由 `RUNTIME_VERSION_UINT64` 承担。照搬「版本号就该改」的直觉会改错。

**消费方影响**：`qaqh-winui-app` 通过 path 依赖指向本仓库工作树，故版本推进对其是**零改动**的。

### 回归验证（本次已做）

- `cargo check`：`windows-reactor` / `windows-reactor-setup` / `test_reactor` / `test_reactor_selftest` / `tool_reactor` 全通过
- 三条 pin 断言手工复核：`RUNTIME_VER == WINDOWS_APP_SDK_VERSION`、WebView2 两处一致、`reactor.yml` URL 匹配
- 端到端：`test_reactor_selftest --list-fixtures` 实跑通过（真实调用 `MddBootstrapInitialize2` 绑定 2.5.1 运行时）
- `qaqh-winui-app` 侧 `just package-winui-desktop` 全流程通过，产出完整运行目录

## 已知陷阱：reactor-setup 缓存中毒（2026-09-21 记录）

`windows-reactor-setup` 的 NuGet 缓存位于 `%LOCALAPPDATA%\windows-reactor-setup\temp`，
有**两个会在下载失败后静默永久失效**的缺陷，排查自包含部署问题时优先看这里：

1. **`.msix_extract` 空目录陷阱**：`ensure_msix_extracted` 只判断「目录是否存在」就跳过解包。
   若首次运行时 MSIX 尚未下好，它会创建**空目录**，此后每次构建都跳过 → 运行时 DLL 永远缺失，
   报错表现为 `target/release/Microsoft.WindowsAppRuntime.dll` not found（`assemble-winui.ps1` 阶段）。
   **处理**：删除空的 `.msix_extract` 目录后重建。
2. **`tar` 解包被中断**：解包中途断流会留下**残缺目录树**（如 `MSIX/` 下只有 `win10-arm64`、`win10-arm64ec`，
   缺 `win10-x64`）。目录存在但内容不全，同样会静默跳过。
   **排查**：用 `[System.IO.Compression.ZipFile]` 列出 nupkg 内的实际条目，与解包结果逐个比对。

**下载本身也不可靠**：2.5.1 运行时包 169 MB，`curl --retry` **不覆盖传输中断**（会「成功」退出但文件截断）。
建议改用 `HttpClient` + `Range` 续传，并校验最终字节数与 `Content-Length` 一致。
历史上 2.4.0 那次「缺 `win10-x64`」即为此因，并非包本身缺文件。

**构建脚本缓存**：修复缓存后，Cargo 可能因判定「无变化」而**不重跑 build script**，需 `touch apps/winui/build.rs`
或删除 `target/<profile>/build/qaqh-winui-*` 强制重跑。

## 同步记录（winui-app 侧重写触发）

- 2026-09-21 **SDK 2.5.1 移植**（cherry-pick 上游 `59cccbe70`）：纯版本号推进，7 文件 / 12 行，无 API 变化；
  winui-app 侧 `package-winui-desktop` 全流程验证通过。
- 2026-08-21 `sidebar` 简化为纯工作区导航（`TabView` 单一切换）+ `home/startup` 工作区选择器，`vendor` 无代码变更，仅刷新快照 rev 供 `qaqh-winui-app` 重新锁定（`1ee42c9 -> next`）。

## 结构

完整保留 fork 工作树（`crates/libs/*`、`crates/tools/*`、根 workspace Cargo.toml），其中被 QAQ-Harness 消费的包：

- `windows-reactor` / `windows-numerics` / `windows`
- `windows-core` / `windows-collections` / `windows-future` / `windows-time`
- `windows-bindgen` / `windows-reactor-setup`

## License

沿用上游 windows-rs 的 MIT/Apache-2.0 许可。
