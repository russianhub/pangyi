# 旁译 Pangyi

**敲中文拼音，在 Windows 输入法候选窗里看俄语释义。** 有重音的数据会显示重音。适合边聊天、写作边查词。

**[下载 Windows 安装包 v4.20.0](https://github.com/russianhub/pangyi/releases/download/v4.20.0/pangyi-v4.20.0.exe)** · [发布与 SHA256](https://github.com/russianhub/pangyi/releases/tag/v4.20.0) · [反馈问题](https://github.com/russianhub/pangyi/issues)

免费开源 · 打字和查词离线运行 · 无广告

## 安装并试用

1. 下载并运行安装包。安装包未签名。若 Windows 显示 SmartScreen 提示，先确认下载来源和发布页提供的 SHA256。
2. 首次安装后按提示注销并重新登录，或重启电脑。升级后保存工作，完全退出再打开使用旁译的应用。若仍加载旧版，请注销后重新登录。
3. 按 `Win + 空格` 选择「旁译」，输入 `pengyou`，在「朋友」候选旁查看俄语释义。双击桌面或开始菜单中的「旁译」可打开设置。

旁译通过 Windows 文本服务框架（TSF）在记事本、Word、浏览器及其他支持系统输入法的应用中使用。

![旁译输入演示，画面为功能示意](docs/hero-v4.18.0.jpg)

| 想做什么 | 这样操作 |
| --- | --- |
| 边打中文边看俄语 | 输入 `pengyou`（朋友）或 `kaifa`（开发） |
| 临时输入英文 | 单独轻按一次 `Shift`，看到中英切换提示后继续输入 |
| 快速输入表情 | 在设置中开启「表情模式」，回到中文模式输入 `xdm`，选择 🤝 |

## v4.20.0 更新

桌面和开始菜单的「旁译」现在打开独立设置窗口 `pangyi-settings.exe`。左侧导航集中管理输入、候选界面和其他选项。切换英文模式和表情模式即时生效。

设置窗口经过 Linux 运行检查和 Windows CI 编译与单元测试。Windows 真机体验仍待手工验证。[完整更新说明](docs/releases/v4.20.0.md)

v4.18.0 加入了[用户词库](docs/user-dictionary.md)和[表情包与符号包](docs/emoji-packs.md)。[v4.18.0 更新说明与验证范围](docs/releases/v4.18.0.md)

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

怎么下载、怎么写一个包给别人，见 [表情包](docs/emoji-packs.md)。

## 反馈与卸载

欢迎在 [Issues](https://github.com/russianhub/pangyi/issues) 用中文反馈。请附 Windows 版本、旁译版本、应用名称、复现步骤；截图前请遮住私人信息。

卸载可运行 `"C:\Program Files\Pangyi\pangyi.exe" --uninstall`，完成后按提示注销或重启。

## 离线使用与可选账号

输入法 DLL 在本机处理打字、查词和表情。独立更新程序自动请求版本信息，可在设置中关闭自动检查。只有你在更新弹窗选择下载时，更新程序才会获取安装包。

设置中的「旁译账号」打开单独的账号窗口，用邮箱验证码登录，不设置密码。没有配置身份服务时，窗口会写明登录不可用，不会显示登录成功。

登录状态用 Windows DPAPI 加密后保存在 `%LOCALAPPDATA%\Pangyi\account.dat`。退出登录会删掉它。词库和按键不会上传。配置方法、隐私说明和 Windows 手工验收见 [邮箱验证码登录](docs/email-login.md)。

## 从源码构建

Windows + Rust（[rustup](https://rustup.rs)）：

```powershell
# 从仓库根目录运行
cargo build --release --lib --manifest-path ime/Cargo.toml  # 64 位 pangyi_ime.dll
# 32 位 DLL：需要 tools/llvm-mingw，并把 build/x86-stub.c 编成 build/x86-stub.o
# 再链进 i686-pc-windows-gnu（做法与 CI 相同，见下一节）
# 安装包 exe：先编更新程序和设置程序，连同两个 DLL、pangyi.ico 放进 ime/payload/，再编 setup
cargo build --release --manifest-path updater/Cargo.toml    # updater/target/release/pangyi-updater.exe
cargo build --release --manifest-path settings/Cargo.toml   # settings/target/release/pangyi-settings.exe
cargo build --release --features account --bin setup --manifest-path ime/Cargo.toml
```

`setup.exe` 用 `include_bytes!` 把 `ime/payload/pangyi-settings.exe` 嵌进去，安装时写到 `C:\Program Files\Pangyi\pangyi-settings.exe`。所以要先编出设置程序，再编安装包。

词库 `ime/src/dict.tsv` 以 `include_str!` 直接编进二进制，没有任何运行时数据文件依赖。`ime/payload/`（DLL、图标、更新程序和设置程序）在 `.gitignore` 里，需要在打安装包之前自己放进去。图标可以用 `python tools/make_ico.py ime/build/icon ime/payload/pangyi.ico` 从 `ime/build/icon/icon-*.png` 生成。

## 持续集成与发布

[![持续集成](https://github.com/russianhub/pangyi/actions/workflows/ci.yml/badge.svg)](https://github.com/russianhub/pangyi/actions/workflows/ci.yml)

`.github/workflows/ci.yml` 在推送到 `main`，以及指向 `main` 的拉取请求上运行，使用 `windows-latest`。

- 64 位：`cargo build --release --lib`。运行器默认是 MSVC 工具链，和本机 rustup 默认宿主上的 `cargo build --release` 一致，产物是 `pangyi_ime.dll`。
- 32 位：下载 [llvm-mingw 20261020](https://github.com/mstorsjo/llvm-mingw/releases/tag/20261020) 的 msvcrt、x86_64 压缩包，核对 SHA256 后解到 `tools/llvm-mingw`（该目录已 gitignore）。用其中的 `i686-w64-mingw32-gcc` 把 `ime/build/x86-stub.c` 编成 `x86-stub.o`，再以 `i686-pc-windows-gnu`、`-C link-self-contained=no`、`+crt-static` 链出 32 位 DLL。
- `cargo test --locked`（`ime` 这个包，含各个 bin 的编译），以及 `cargo test --locked --manifest-path updater/Cargo.toml`。`dict.tsv`、`lm.tsv` 直接在 Git 里，不是 Git LFS；checkout 用默认设置即可。
- 用 `tools/make_ico.py` 从 `ime/build/icon/icon-*.png` 生成 `pangyi.ico`，把两个 DLL 放进 `ime/payload/`。64 位 `pangyi-settings.exe` 先编出来，复制进 `ime/payload/`，再 `cargo build --release --bin setup`。CI 会在日志里打印 `pangyi-settings.exe` 和 `setup.exe` 的字节数。这一步只在 CI 里验证，不上传文件。

32 位链接在工具链目录里补两样东西，不改仓库里的源文件，也不提交生成的对象：

- `x86-stub.o` 提供 `__register_frame_info` 这一组空实现。`i686-pc-windows-gnu` 的 `rsbegin.o` / `rsend.o` 会引用它们，llvm-mingw 的 libunwind 没有这些符号。
- rustc 还会传 `-lgcc`、`-lgcc_eh`。llvm-mingw 没有这两个文件名，脚本把一个占位对象做成 `libgcc.a`，并把静态 `libunwind.a` 复制为 `libgcc_eh.a`。帧注册符号仍然来自 stub。`+crt-static` 用来把 unwind 静态链进去，安装包不依赖 `libunwind.dll`。

只编 DLL、旁边还没有 `ime/payload/pangyi.ico` 时，`build.rs` 会警告「payload/pangyi.ico 不存在，跳过资源编译」然后继续。这是预期行为。图标和 DLL 都不要提交。

Cargo 注册表和 `ime/target` 走 Actions 缓存；llvm-mingw 另有一份缓存。

`.github/workflows/release.yml` 在推送 `v*` 标签（例如 `v4.18.0`）时跑同一套构建，把安装包命名为 `pangyi-<标签>.exe`，并写一个 `sha256sum -c` 能用的 `.sha256` 文件。然后用 `gh release create --draft` 把这两个文件上传到该标签的草稿 Release。说明正文用 `docs/releases/<标签>.md`（`v4.18.0` 就是 `docs/releases/v4.18.0.md`）；没有这个文件，工作流失败，不创建 Release。草稿要人在 GitHub 上核对校验值后再公开。这个标签如果已经有 Release（含草稿），工作流会失败并留下原来的附件。用的是 Actions 自带的 `GITHUB_TOKEN`，不用另配 secret。权限是 `contents: write`（创建 Release）和 `actions: write`（写缓存）。版本号不写在工作流里：安装包里的版本来自 `ime/Cargo.toml`。release workflow 会把
`updater/Cargo.toml` 和 `updater/Cargo.lock` 里 `pangyi-updater` 的版本号，以及 `settings/Cargo.toml` 和 `settings/Cargo.lock` 里 `pangyi-settings` 的版本号，同步成标签后再构建。已经存在的标签和 Release 不会被改写。

把草稿 Release 点「发布」后，网站仓库（russianhub/opensource-sites）的
`pangyi-manifest` 同步工作流（每小时，或手动触发）会下载安装包、计算 SHA256 并用
ed25519 签名，把 `latest.json` 放进网站发布目录并重新部署。清单的主下载地址约定为
Cloudflare R2 直链 `https://download.eyuxq.com/pangyi-v<版本>.exe`（`url`），
GitHub Release 直链写在 `mirrors` 里兜底；安装包不进网站仓库。eyuxq.com 上的下载按钮
读这份清单，已安装用户下次登录会收到更新提醒。输入法 DLL 不参与这次请求。
网站仓库要配的 Secrets，以及工作流还没改到 R2 时的现状，见 [updater/README.md](updater/README.md)。

## 许可

- 代码：MIT（见 [LICENSE](LICENSE)）。`pangyi-settings.exe` 的源码同样是 MIT。
- 设置程序链接的 Slint 按免版税协议 LicenseRef-Slint-Royalty-free-2.0 使用。Slint 1.19 的许可证是 GPLv3、免版税协议、商业许可三选一，本仓库选免版税这一条。关于页放有 “Made with Slint” 署名。输入法 DLL 按 MIT 分发。安装时设置程序写到 `C:\Program Files\Pangyi\pangyi-settings.exe`。
- 数据：CC BY-SA 系列 —— 分发时必须保留 [NOTICE.md](NOTICE.md) 并以相同方式共享。生成步骤见 [数据流水线](docs/data-pipeline.md)，常用词释义抽查见 [释义抽查](docs/gloss-audit.md)。手写覆盖表 `data/gloss-overrides.tsv` 是原创，同样按 CC BY-SA 随数据分发
- 用户自己写的 `user-dict.tsv` / `user-gloss.tsv` 只在用户电脑上，不随安装包分发，也不纳入上面的 CC BY-SA 数据
