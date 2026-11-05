# 旁译 Pangyi

### 敲拼音，看俄语。把查词融入每一次输入。

![旁译 v4.17.0：中文输入、俄语释义、本机运行；画面为功能示意](docs/hero-v4.17.0.jpg)

**面向俄语学习者的 Windows 系统输入法。** 输入熟悉的中文拼音，在候选窗里查看对应的俄语释义；有重音的数据会显示重音。写作、聊天、记单词，都从一次输入开始。

**[↓ 下载 v4.17.0 Windows 安装包](https://github.com/russianhub/pangyi/releases/download/v4.17.0/pangyi-v4.17.0.exe)** · [更新说明](docs/releases/v4.17.0.md) · [反馈问题](https://github.com/russianhub/pangyi/issues)

免费开源 · 本机运行 · 无广告 · 无需联网查词

## 先试这三个输入

| 你想做什么 | 这样操作 |
| --- | --- |
| 边打中文，边看俄语 | 输入 `pengyou`，在“朋友”旁查看俄语释义 |
| 聊天时快速选表情 | 设置中开启表情模式，回到中文模式输入 `xdm`，候选中出现 🤝 |
| 临时输入英文 | 单独轻按一下 `Shift`，看到中/英模式切换提示后继续输入 |

旁译接入 Windows 输入法系统（TSF），安装后按 `Win + 空格` 切换使用，无需每次启动独立查词程序。可在记事本、Word、浏览器、聊天软件等支持系统输入法的应用中使用；具体兼容情况欢迎反馈。

## v4.17.0：输入更干净，常用词俄语更正

- **字母、确认和点选更听话**：只有主键盘 A–Z 进拼音；小键盘和功能键交给应用。没有候选时，Tab 和上下方向键交给应用，回车和空格上屏原文，且不写入自学习。点选只落在画出的词和释义上。
- **候选窗跟着副屏走**：按光标所在显示器的工作区摆放，不再被夹回主屏，也不钻进任务栏。
- **多音字上屏用当前这一行**：不再按汉字去词典里另找一个读音。
- **127 条手写俄语覆盖**：按「简体 + 读音」整条换掉高频错译，例如「的」「没」「事业」「承诺」，以及「长」的两个读音。`dict.tsv` 没有改。
- **抽查和发布流程写进文档**：前 5,000 常用词的释义抽查、数据怎样编进输入法，以及推送 `v*` 标签后生成草稿安装包。

[查看完整更新、重启说明与尚未在 Windows 上手工验证的部分 →](docs/releases/v4.17.0.md)

## 三步开始

1. 下载并运行上方 `pangyi-v4.17.0.exe` 安装包。该版本未签名；若出现 SmartScreen 提示，先确认下载来源与 [Release 中的 SHA256](https://github.com/russianhub/pangyi/releases/tag/v4.17.0)，再决定是否继续运行。
2. 首次安装后，按安装提示注销并重新登录（也可重启电脑）；升级后，保存工作并完全退出、重新打开使用旁译的应用，让它加载新版输入法。
3. 按 `Win + 空格` 选择「旁译」，输入 `pengyou` 或 `kaifa` 开始使用。双击桌面「旁译」打开设置。

> **什么时候需要重启？** 安装或升级涉及输入法组件加载；日常切换英文模式、表情模式无需重启应用或电脑。热重载仅覆盖这两个开关。如果升级后仍像旧版，先完全退出应用（含后台进程）再打开；仍无效时注销后重新登录。

## 常用操作

| 操作 | 用法 |
| --- | --- |
| 切换到旁译 | `Win + 空格` |
| 全拼 / 缩写 | `nv` → 女，`lvxing` → 旅行；支持首字母缩写，如 `gz` → 工作 |
| 选择候选 | `↑` / `↓` 移动，空格确认当前候选 |
| 翻页 | `-` / `=` 或 `PageUp` / `PageDown`；上下移动到页边界时可跨页 |
| 修改拼音中间的错字 | `←` / `→` 移动插入位置；`Home` / `End` 到两端；退格 / Delete 删除 |
| 选择俄语义项（竖排） | 拼音光标在最右时再按 `→` 进入俄语侧，继续 `→` 切换义项，`←` 返回中文侧；空格确认 |
| 中英切换 / 大写 | 单独轻按 `Shift` 切中英；`Caps Lock` 控制大写 |
| 使用表情 | 设置 →「表情模式」开；在中文模式输入完整关键词，选择表情候选后确认 |
| 调整界面 | 双击桌面「旁译」，设置候选个数、字号、横竖排、主题等 |

表情模式关闭后不注入表情候选；表情不会进入自学习词库。表情是 Unicode 字符和颜文字，显示效果受 Windows 字体影响，不是图片贴纸。

## 本地词库与数据来源

- 当前中文词典 **457,138 条记录**；其中有俄语译文的条目显示释义，并非每个中文词都已有俄语译文。
- 数据来自 CC-CEDICT、俄语维基词典（含 kaikki 英文维基枢轴）、Wikdict、FreeDict、UniHan、jieba 等免费来源，未使用付费翻译 API。高频词里已经核对过的错译，用 [`data/gloss-overrides.tsv`](data/gloss-overrides.tsv) 在加载时整条换掉俄语；这份表是原创，随数据按 CC BY-SA 分发，不替代下面的上游署名。
- 词典释义适合辅助查词与学习；多义词需结合语境判断。
- 数据文件怎样生成，见 [数据流水线](docs/data-pipeline.md)。常用词俄语释义的覆盖率和抽查，见 [释义抽查](docs/gloss-audit.md)。
- 数据署名与分发要求见 [NOTICE.md](NOTICE.md)。

灵感来自 [青简 Qingjian](https://qingjian.app)，旁译专注中文 → 俄语。

## 自己的词和俄语

安装包里的词库不能直接改。要加自己的词，或者换掉某个词的俄语，在本机这两个文本文件里写，不用联网，也不用账号：

- `%LOCALAPPDATA%\Pangyi\user-dict.tsv` — 添加词（词头、拼音、可选俄语、可选权重）
- `%LOCALAPPDATA%\Pangyi\user-gloss.tsv` — 改已有词的俄语

这两个文件和自定义短语、自学习记在同一个文件夹。第一次切到旁译时，如果文件还不存在，会生成带说明的模板。它们只留在你的电脑上，不会上传，也不属于下面 CC BY-SA 词典数据。格式、权重怎么排、以及怎么打开文件夹，见 [用户词库](docs/user-dictionary.md)。

## 表情包和符号包

常用表情、颜文字、标点、数学符号，以及 «»、№、₽ 这类俄语符号，可以做成一份「包」放在本机：

```text
%LOCALAPPDATA%\Pangyi\packs\
```

放进这个文件夹的 `*.tsv` 会在下一次打拼音时生效，删掉文件就关闭。官方包用仓库里的 `scripts/pangyi-packs.ps1` 下载，并核对 SHA256。

输入法只读已经在本机的文件。查词和打字都在你的电脑上完成。下载是另一个脚本做的事；不运行这个脚本，输入法就不会去访问网络。这是为了保住「全本机、无联网」：输入法 DLL 里没有下载代码，也不会在你打字时自己去取表情包。

怎么下载、怎么写一个包给别人，见 [表情包](docs/emoji-packs.md)。

## 反馈与卸载

欢迎在 [Issues](https://github.com/russianhub/pangyi/issues) 用中文反馈。请附 Windows 版本、旁译版本、应用名称、复现步骤；截图前请遮住私人信息。

卸载可运行 `"C:\Program Files\Pangyi\pangyi.exe" --uninstall`，完成后按提示注销或重启。

## 从源码构建

Windows + Rust（[rustup](https://rustup.rs)）：

```powershell
cd ime
cargo build --release                    # 64 位 pangyi_ime.dll（cdylib）
# 32 位 DLL：需要 tools/llvm-mingw，并把 build/x86-stub.c 编成 build/x86-stub.o
# 再链进 i686-pc-windows-gnu（做法与 CI 相同，见下一节）
# 安装包 exe：把两个 DLL 和 pangyi.ico 放进 ime/payload/ 后
cargo build --release --bin setup        # ime/target/release/setup.exe
```

词库 `ime/src/dict.tsv` 以 `include_str!` 直接编进二进制，没有任何运行时数据文件依赖。`ime/payload/`（DLL 和图标）在 `.gitignore` 里，需要在打安装包之前自己放进去。图标可以用 `python tools/make_ico.py ime/build/icon ime/payload/pangyi.ico` 从 `ime/build/icon/icon-*.png` 生成。

## 持续集成与发布

[![持续集成](https://github.com/russianhub/pangyi/actions/workflows/ci.yml/badge.svg)](https://github.com/russianhub/pangyi/actions/workflows/ci.yml)

`.github/workflows/ci.yml` 在推送到 `main`，以及指向 `main` 的拉取请求上运行，使用 `windows-latest`。

- 64 位：`cargo build --release --lib`。运行器默认是 MSVC 工具链，和本机 rustup 默认宿主上的 `cargo build --release` 一致，产物是 `pangyi_ime.dll`。
- 32 位：下载 [llvm-mingw 20261020](https://github.com/mstorsjo/llvm-mingw/releases/tag/20261020) 的 msvcrt、x86_64 压缩包，核对 SHA256 后解到 `tools/llvm-mingw`（该目录已 gitignore）。用其中的 `i686-w64-mingw32-gcc` 把 `ime/build/x86-stub.c` 编成 `x86-stub.o`，再以 `i686-pc-windows-gnu`、`-C link-self-contained=no`、`+crt-static` 链出 32 位 DLL。
- `cargo test --locked`（`ime` 这个包，含各个 bin 的编译）。`dict.tsv`、`lm.tsv` 直接在 Git 里，不是 Git LFS；checkout 用默认设置即可。
- 用 `tools/make_ico.py` 从 `ime/build/icon/icon-*.png` 生成 `pangyi.ico`，把两个 DLL 放进 `ime/payload/`，再 `cargo build --release --bin setup`，确认安装包能编过。这一步只在 CI 里验证，不上传文件。

32 位链接在工具链目录里补两样东西，不改仓库里的源文件，也不提交生成的对象：

- `x86-stub.o` 提供 `__register_frame_info` 这一组空实现。`i686-pc-windows-gnu` 的 `rsbegin.o` / `rsend.o` 会引用它们，llvm-mingw 的 libunwind 没有这些符号。
- rustc 还会传 `-lgcc`、`-lgcc_eh`。llvm-mingw 没有这两个文件名，脚本把一个占位对象做成 `libgcc.a`，并把静态 `libunwind.a` 复制为 `libgcc_eh.a`。帧注册符号仍然来自 stub。`+crt-static` 用来把 unwind 静态链进去，安装包不依赖 `libunwind.dll`。

只编 DLL、旁边还没有 `ime/payload/pangyi.ico` 时，`build.rs` 会警告「payload/pangyi.ico 不存在，跳过资源编译」然后继续。这是预期行为。图标和 DLL 都不要提交。

Cargo 注册表和 `ime/target` 走 Actions 缓存；llvm-mingw 另有一份缓存。

`.github/workflows/release.yml` 在推送 `v*` 标签（例如 `v4.17.0`）时跑同一套构建，把安装包命名为 `pangyi-<标签>.exe`，并写一个 `sha256sum -c` 能用的 `.sha256` 文件。然后用 `gh release create --draft` 把这两个文件上传到该标签的草稿 Release。说明正文用 `docs/releases/<标签>.md`（`v4.17.0` 就是 `docs/releases/v4.17.0.md`）；没有这个文件，工作流失败，不创建 Release。草稿要人在 GitHub 上核对校验值后再公开。这个标签如果已经有 Release（含草稿），工作流会失败并留下原来的附件。用的是 Actions 自带的 `GITHUB_TOKEN`，不用另配 secret。权限是 `contents: write`（创建 Release）和 `actions: write`（写缓存）。版本号不写在工作流里：安装包里的版本来自 `ime/Cargo.toml`。

## 许可

- 代码：MIT（见 [LICENSE](LICENSE)）
- 数据：CC BY-SA 系列 —— 分发时必须保留 [NOTICE.md](NOTICE.md) 并以相同方式共享。生成步骤见 [数据流水线](docs/data-pipeline.md)，常用词释义抽查见 [释义抽查](docs/gloss-audit.md)。手写覆盖表 `data/gloss-overrides.tsv` 是原创，同样按 CC BY-SA 随数据分发
- 用户自己写的 `user-dict.tsv` / `user-gloss.tsv` 只在用户电脑上，不随安装包分发，也不纳入上面的 CC BY-SA 数据
