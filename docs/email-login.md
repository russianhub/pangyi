# 邮箱验证码登录

旁译桌面主程序支持 Supabase 邮箱一次性验证码（OTP）登录。输入法 DLL 不包含认证
入口，也不会在打字时联网。登录不是使用本地输入法功能的前提。

## Supabase 免费项目

1. 创建 Supabase 项目，在 Authentication → Providers 中启用 Email。
2. 把 Magic Link 邮件模板改为显示 `{{ .Token }}`，让邮件包含 6 位验证码。
3. 构建前提供两个环境变量：

```powershell
$env:PANGYI_SUPABASE_URL = 'https://你的项目.supabase.co'
$env:PANGYI_SUPABASE_PUBLISHABLE_KEY = 'sb_publishable_...'
```

也兼容旧式 `anon` key。这里只能使用 publishable/anon 客户端 key，绝不能使用
`service_role` key。两个值都会进入客户端二进制，不应被当作服务端秘密。

未提供配置时仍可正常构建和使用输入法；账号窗口会明确显示“登录服务尚未配置”。
GitHub Actions 会从同名的 Repository variables 读取它们；创建 Supabase 项目后，在
仓库 Settings → Secrets and variables → Actions → Variables 中填写即可。
登录成功后，refresh token 经 Windows DPAPI 加密后保存到
`%LOCALAPPDATA%\Pangyi\account.dat`，只有当前 Windows 用户能解开。

Supabase 默认限制同一用户 60 秒内只能请求一次 OTP；界面会提示用户等待。
