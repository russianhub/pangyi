# 旁译更新提醒 · pangyi-updater

输入法本体（DLL）永远不联网。更新提醒是**独立程序**：

- `src/main.rs`：更新提醒程序，编出 `pangyi-updater.exe`
- 开机自启（HKCU Run，当前用户）：`pangyi-updater.exe --silent`
- 设置面板「更新」行：「检查更新」→ `--manual`；「自动/关」→ `--enable` / `--disable`
- 检查地址：`https://eyuxq.com/latest.json`（ed25519 签名校验，SHA256 校验安装包）

## 组件关系

| 文件 | 位置 | 作用 |
| --- | --- | --- |
| `updater/`（本目录） | pangyi 主仓库 | 更新程序源码，随 Release 一起构建并打进安装包 |
| `tools/make_manifest.py` | pangyi 主仓库 | 发版时生成并签名 latest.json |
| `.github/workflows/publish-manifest.yml` | pangyi 主仓库 | 点「发布 Release」后，把安装包和 latest.json 推到网站仓库 |
| `dist/pangyi-dl.js` | 网站仓库 | 网站下载按钮：从 latest.json 读版本/日期/SHA256 |

## 一次性配置

1. **签名密钥**（已生成过就跳过）：
   ```
   openssl genpkey -algorithm ed25519 -out pangyi-sign.pem
   openssl pkey -in pangyi-sign.pem -pubout -outform DER | tail -c 32 | base64
   ```
   公钥填到 `src/main.rs` 的 `PUBLIC_KEY_B64`；私钥 `pangyi-sign.pem` **不要提交**，单独备份。
   丢了就无法给老用户推送更新。

2. **GitHub Secrets**（pangyi 主仓库 → Settings → Secrets and variables → Actions）：
   - `PANGYI_SIGN_KEY`：`pangyi-sign.pem` 全文
   - `SITE_REPO_TOKEN`：fine-grained token，只授权网站仓库（russianhub/opensource-sites）Contents 读写

## 发版流程

1. 推 `v*` 标签 → CI 构建安装包和更新程序，创建**草稿** Release。
   release workflow 会先把 `updater/Cargo.toml` 的版本号同步成标签（保证更新程序
   报告的「当前版本」与输入法一致）。
2. 在 Release 说明里写更新内容（每行一条，前 8 行会显示在更新弹窗里）。
3. 点「发布」→ `publish-manifest.yml` 自动：下载安装包 → 算 SHA256 并签名 →
   把 `pangyi-vX.Y.Z.exe`、`pangyi-latest.exe`、`latest.json` 推到网站仓库的
   `dist/download/` 和 `dist/latest.json`（保留最近两个版本）→ 网站自动重新部署。

## 注意

- 已安装的老版本没有更新程序，需要手动装一次带更新程序的版本。
- 网站仓库每发一版会多约一个安装包大小的历史体积；如果仓库明显变大，把安装包
  改放对象存储（如 Cloudflare R2），只需改 workflow 里的上传步骤和 `DOWNLOAD_BASE`。
- 官网文案：输入法本体完全离线；更新检查是独立组件，只请求版本信息，可在设置里关闭。
