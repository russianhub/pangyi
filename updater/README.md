# 旁译更新提醒 · pangyi-updater

输入法本体（DLL）永远不联网。更新提醒是**独立程序**：

- `src/main.rs`：更新提醒程序，编出 `pangyi-updater.exe`
- 开机自启（HKCU Run，当前用户）：`pangyi-updater.exe --silent`
- 设置面板「更新」行：「检查更新」→ `--manual`；「自动/关」→ `--enable` / `--disable`
- 检查地址：`https://eyuxq.com/latest.json`（ed25519 签名校验，SHA256 校验安装包）

自动检查只请求这份版本清单。用户在弹窗里点「是：现在下载并安装」之后，程序才会去下载安装包。这两步都不在输入法 DLL 里。设置里的「更新」行可以随时关掉自动检查。

## 下载：Cloudflare R2 主链，GitHub Release 兜底

更新程序自己不决定安装包放在哪，只按 `latest.json` 里的地址依次尝试：

1. 先验 ed25519 签名。签名明文与 `tools/make_manifest.py`、网站仓库的 `pangyi-make-manifest.py` 相同：

   ```text
   pangyi
   <version>
   <sha256，去掉首尾空白并转成小写>
   <size>
   ```

   签名盖住版本号、SHA256 和大小。`url` / `mirrors` 不在签名里；真正落盘前会再核对字节数和 SHA256，对不上就丢掉。
2. 再按顺序下载：先清单里的 `url`，然后 `mirrors` 里的地址。不是 `https://` 开头的直接跳过。
3. `url` 是 Cloudflare R2 直链：`https://download.eyuxq.com/pangyi-v<版本>.exe`。
4. `mirrors` 放 GitHub Release 直链，例如 `https://github.com/russianhub/pangyi/releases/download/v<版本>/pangyi-v<版本>.exe`。主地址连不上、或下下来校验失败时，按数组顺序自动换下一个。
5. 清单里的 `size` 为 0，或大于 500MB（`MAX_INSTALLER_BYTES`），直接拒绝，不会开始下载。

## 网站仓库要配的 Secrets

安装包上传和 `latest.json` 签名在 `russianhub/opensource-sites` 的 `pangyi-manifest` 工作流里做，不在本仓库，也不在输入法 DLL 里。下面只列名字，**不要把值写进 git**：

| 名字 | 用途 |
| --- | --- |
| `R2_ACCOUNT_ID` | Cloudflare 账号 ID |
| `R2_ACCESS_KEY_ID` | R2 API 令牌的 Access Key ID |
| `R2_SECRET_ACCESS_KEY` | R2 API 令牌的 Secret Access Key |
| `R2_BUCKET` | 存放 `pangyi-v<版本>.exe` 的桶 |
| `PANGYI_SIGN_KEY` | ed25519 私钥 PEM，用来签 `latest.json`。对应本目录 `PUBLIC_KEY_B64` |
| `PANGYI_DOWNLOAD_BASE`（可选） | 主下载基址。不设时用 `https://download.eyuxq.com`，清单 `url` 为 `{基址}/pangyi-v<版本>.exe` |

工作流还要把对应的 GitHub Release 资源地址写进 `mirrors`，R2 失败时客户端才能自动兜底。

签名脚本当前读的变量名是 `DOWNLOAD_BASE` 和 `MIRROR_URL`（见本仓库 `tools/make_manifest.py`；`DOWNLOAD_BASE` 未设置时默认 `https://eyuxq.com/download`）。网站工作流应把 `PANGYI_DOWNLOAD_BASE` 传给 `DOWNLOAD_BASE`，把 GitHub Release 直链传给 `MIRROR_URL`。

撰写本文时，`opensource-sites` 的 `main` 仍把 `DOWNLOAD_BASE` 设成 GitHub Release 目录，还没有读取上表里的 R2 Secrets。客户端已经按「`url` 优先、`mirrors` 兜底」实现；线上 `latest.json` 要等该工作流接上这些配置，主地址才会变成 `download.eyuxq.com`。

## 组件关系

| 组件 | 位置 | 作用 |
| --- | --- | --- |
| `updater/`（本目录） | pangyi 主仓库 | 更新程序源码，随 Release 一起构建并打进安装包 |
| `docs/releases/<标签>.md` | pangyi 主仓库 | 发布说明（前 8 行会显示在更新弹窗里） |
| `.github/workflows/pangyi-manifest.yml` | 网站仓库 russianhub/opensource-sites | 每小时查一次 pangyi 的最新正式 Release，有新版本就下载安装包、算 SHA256、签名并推 `dist/latest.json`，随后自行部署 Pages。R2 上传和 `url` / `mirrors` 怎么填，见上文「Secrets」。安装包不进网站仓库 |
| `.github/scripts/pangyi-make-manifest.py` | 网站仓库 | 生成并签名 latest.json。`url` 来自下载基址，`mirrors` 来自 `MIRROR_URL` |
| `dist/pangyi-dl.js` | 网站仓库 | 网站下载按钮：从 latest.json 读版本/日期/SHA256 |

## 签名密钥（一次性）

```
openssl genpkey -algorithm ed25519 -out pangyi-sign.pem
openssl pkey -in pangyi-sign.pem -pubout -outform DER | tail -c 32 | base64
```

- 公钥（第二条命令的输出）填到 `src/main.rs` 的 `PUBLIC_KEY_B64`。
- 私钥 `pangyi-sign.pem` **不要提交**，单独备份；设成网站仓库的 Actions secret
  `PANGYI_SIGN_KEY`（网站同步工作流用它签名）。丢了就无法给老用户推送更新。

## 发版流程

1. 写 `docs/releases/<版本>.md` 并 bump 版本号（`ime/Cargo.toml`、`account/Cargo.toml`、
   `updater/Cargo.toml` 以及两个 Cargo.lock 的根包条目）。
2. 推 `v*` 标签 → CI 构建安装包和更新程序（release workflow 会把 updater 的版本号
   同步成标签，并改 `updater/Cargo.lock` 里对应的根包版本，好让 `--locked` 测试通过），创建**草稿** Release。
3. 在 GitHub 上核对校验值后点「发布」。
4. 其余由网站仓库完成：同步工作流（每小时，或手动 Run workflow）下载安装包、计算 SHA256
   并签名，把 latest.json 推到 eyuxq.com 并重新部署。Secrets 配好之后，安装包上传到
   Cloudflare R2，`url` 为 `https://download.eyuxq.com/pangyi-v<版本>.exe`，`mirrors`
   为 GitHub Release。官网下载按钮和已安装用户的更新提醒都读这份清单。

## 测试

版本比较、签名校验、下载地址顺序和安装包大小上下界都有单元测试，不依赖 Windows：

```
cargo test --locked --manifest-path updater/Cargo.toml
```

Windows CI（`.github/scripts/build-windows.ps1`）在编 DLL 之前跑同一条命令。输入法 DLL 的离线检查（`Assert-ImeStaysOffline`）和 64 位 / 32 位构建检查保持原样。

## 注意

- 已安装的老版本没有更新程序，需要手动装一次带更新程序的版本。
- 安装包不进网站仓库。主链是 Cloudflare R2（`download.eyuxq.com`），GitHub Release 只做镜像。
- 官网文案：输入法本体完全离线；更新检查是独立组件，只请求版本信息，可在设置里关闭。
