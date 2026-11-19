# 旁译更新提醒 · pangyi-updater

输入法本体（DLL）永远不联网。更新提醒是**独立程序**：

- `src/main.rs`：更新提醒程序，编出 `pangyi-updater.exe`
- 开机自启（HKCU Run，当前用户）：`pangyi-updater.exe --silent`
- 设置面板「更新」行：「检查更新」→ `--manual`；「自动/关」→ `--enable` / `--disable`
- 检查地址：`https://eyuxq.com/latest.json`（ed25519 签名校验，SHA256 校验安装包）

## 组件关系

| 组件 | 位置 | 作用 |
| --- | --- | --- |
| `updater/`（本目录） | pangyi 主仓库 | 更新程序源码，随 Release 一起构建并打进安装包 |
| `docs/releases/<标签>.md` | pangyi 主仓库 | 发布说明（前 8 行会显示在更新弹窗里） |
| `.github/workflows/pangyi-manifest.yml` | 网站仓库 russianhub/opensource-sites | 每小时查一次 pangyi 的最新正式 Release，有新版本就下载安装包、签名并推 `dist/latest.json` 和 `dist/download/`，网站随之自动更新 |
| `.github/scripts/pangyi-make-manifest.py` | 网站仓库 | 生成并签名 latest.json |
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
   同步成标签），创建**草稿** Release。
3. 在 GitHub 上核对校验值后点「发布」。
4. 其余自动完成：网站仓库的同步工作流（每小时，或手动 Run workflow）会把安装包和
   签名清单推到 eyuxq.com，官网下载按钮更新，已安装用户下次登录收到提醒。

## 注意

- 已安装的老版本没有更新程序，需要手动装一次带更新程序的版本。
- 网站仓库每发一版会多约一个安装包大小的历史体积；如果仓库明显变大，把安装包
  改放对象存储（如 Cloudflare R2），只需改同步工作流里的上传步骤和 `DOWNLOAD_BASE`。
- 官网文案：输入法本体完全离线；更新检查是独立组件，只请求版本信息，可在设置里关闭。
