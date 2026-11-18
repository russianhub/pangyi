# 邮箱验证码登录

打字、词库和表情包都不需要登录。输入法 DLL 不访问网络。账号是桌面主程序里的另一个窗口：设置里点「旁译账号」，会再打开一个「旁译账号」窗口。只有在这个窗口里发送验证码、登录、刷新或退出时，它才会联网。

已发布的 v4.18.0 安装包是在这个窗口写好之前打的，里面没有账号入口。要从源码装一版带账号窗口的程序，按仓库里的 Windows 构建流程编 `setup.exe`。

这次没有云同步，没有付费，也没有社交功能。窗口不会读取或上传按键、候选、词库、表情包或用户文件。发出去的只有邮箱地址，以及你填进窗口的验证码。

## 用户怎么登录

1. 双击桌面「旁译」，打开设置。
2. 点「旁译账号」里的「邮箱验证码登录」。这会新开一个窗口，不会在记事本或浏览器里弹窗，也不会卡住正在打的字。
3. 输入邮箱，点「发送验证码」。邮件里是一串数字，不是密码。
4. 把数字填进去，点「登录」。成功后窗口写「已登录」。
5. 点「退出登录」会删掉本机保存的会话。网络不通时，本机登录状态仍然会清掉，服务器上的会话要等它自己过期。

没有配置登录服务时，窗口写明「登录不可用」，按钮不能点，也不会显示登录成功。

验证码在窗口里最多认 10 分钟。60 秒内不能连着发送，这个等待是真的会挡住按钮。点「取消」会丢掉这一次还没完成的登录，迟到的结果不会被当成登录成功。

## 登录状态放在哪

刷新令牌和访问令牌用当前 Windows 用户的 DPAPI 加密后，写在：

```text
%LOCALAPPDATA%\Pangyi\account.dat
```

文件开头是 `PYAC1`，后面是密文。令牌不以明文写进 `settings.txt` 或别的文本配置。退出登录时先把文件盖成 0 再删除。

DPAPI 只保护到「这台电脑上的这个 Windows 用户」。同一用户下的其他程序如果故意去解密，系统允许它解开。这不是用来防本机恶意程序的。

上一版如果已经留下 `email=` 开头的文件，还能读出里面的刷新令牌。新的登录会改写成上面的格式。解不开的文件不会显示成已登录。

## 维护者要准备什么

身份服务用 [Supabase Auth](https://supabase.com/docs/guides/auth/auth-email-passwordless)（GoTrue 的邮箱一次性验证码）。旁译不自建密码，也没有服务端密钥。

1. 创建一个 Supabase 项目。在 Authentication → Providers 里打开 Email。
2. 把 Magic Link / 确认邮件模板改成显示出 `{{ .Token }}`，让用户收到数字验证码，而不是只能点链接。可以用这样的正文：

```html
<h2>旁译登录验证码</h2>
<p>验证码：{{ .Token }}</p>
<p>如果不是你本人操作，忽略这封邮件即可。</p>
```

3. 在 Authentication → Email 里把 OTP 有效期设成不超过 1 小时。客户端自己只认 10 分钟。
4. 复制项目的 URL，形如 `https://<项目>.supabase.co`，以及 **anon** 或 **publishable** key。不要复制 `service_role` 或 `sb_secret_`。
5. 如果项目会拦截没有验证码的注册，保持邮箱注册可用，或者事先在后台建好用户并把配置里的 `create_user` 设成 `false`。
6. Supabase 自带的发信额度很小。要给真实用户用，在项目里另配 SMTP。SMTP 密码只留在 Supabase，不要写进旁译。
7. 关掉必须先过验证码人机校验的设置。这个桌面窗口没有人机校验，开了就会登录失败，而不会绕过它。

把配置写成下面这个文件。两个位置都可以，程序先读第一个已经存在的文件，不再看后面的：

```text
%ProgramData%\Pangyi\account.toml
```

或者和 `pangyi.exe` 放在一起：

```text
C:\Program Files\Pangyi\account.toml
```

安装程序会放一份空的 `account.toml.example`，里面没有地址。空的地址不能登录。

```toml
provider = supabase
url = https://你的项目.supabase.co
anon_key = 这里填 anon 或 sb_publishable
create_user = true
```

`url` 必须是 `https`，不能带账号密码，路径只能是空的或 `/auth/v1`。程序不跟随跳转。证书用 Windows 自己的证书库校验，不关闭校验。系统代理可以走，但 TLS 仍然要校验。

也可以在**编译主程序之前**设置环境变量，效果和写进文件一样。没写文件时才用环境变量：

```powershell
$env:PANGYI_SUPABASE_URL = 'https://你的项目.supabase.co'
$env:PANGYI_SUPABASE_PUBLISHABLE_KEY = 'sb_publishable_或_anon_key'
```

这两个值会进客户端二进制。它们是公开的客户端标识，不是秘密，但仍然不要把 `service_role` 编进去。程序若发现 role 是 `service_role` 或 key 以 `sb_secret_` 开头，会拒绝登录。

GitHub Actions 会把同名的 Repository variables 传给构建。不填的话，编出来的程序就是「登录不可用」。不要为了让 CI 变绿去创建 Supabase 项目或往变量里放真实密钥。

## 为什么不是 PKCE

邮箱验证码不是 OAuth 授权码流程：没有浏览器跳转，也就没有 `code_verifier`。对应的约束是：

- 只访问配置里的 `https` 地址，关掉 WinHTTP 重定向。
- 每次发送验证码有一个随机 flow id。验证必须带上同一次的邮箱和这个 id。
- 取消、再次发送，或超过 10 分钟之后，旧结果不会被保存成登录成功。若验证请求已经在服务端换到了令牌，窗口会再尝试通知服务器作废，并且不写入本机。
- 验证码是否正确由 Supabase 判断。客户端不接受「没有令牌的 200」当作登录成功。

## 手工验收

在一台真实的 Windows 上逐项看：[账号窗口手工验收](email-login-windows-checklist.md)。

自动化测试用假的 HTTP 响应，不会发真邮件，也不会创建 Supabase 项目。
