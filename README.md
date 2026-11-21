# DeskLink

DeskLink 是一个面向局域网的键盘、鼠标和剪贴板协同工具（软 KVM），不传输桌面画面。支持两种组合：

- **Windows → Windows**：`desklink-windows.exe` 作为主机（Host，接键鼠的那台），`desklink-windows-client.exe` 作为被控端（Client，另一台 Windows）。
- **Windows → Linux**：`desklink-windows.exe` + `desklink-linux`。

当前版本：`0.7.0`。

## 功能

- Windows 捕获真实键盘、鼠标输入。
- Windows Client 通过 `SendInput` 注入键盘、按键、滚轮，并用绝对定位放置光标。
- Linux Client 通过 `/dev/uinput` 创建虚拟键盘和虚拟鼠标。
- UDP 传输高频输入，带序列号、会话校验、共享令牌和 ACK。
- 多显示器支持不同分辨率、缩放比例、负坐标和不规则排列。
- 远端作为一个逻辑屏幕加入屏幕拼图。
- 鼠标通过有效 EdgeLink 自动进入远端，并可从对应边缘返回。
- 两端可以任意顺序启动。
- `Ctrl+Alt+Esc` 紧急恢复 Windows 本地输入。
- localhost 页面拖动、保存远端屏幕位置。
- 双向同步 UTF-8 纯文本剪贴板。

## 工程目录

```text
DeskLink/
├─ Cargo.toml
├─ config/          配置模型
├─ protocol/        两端通信协议
├─ topology/        ScreenRect 与 EdgeLink 算法
├─ windows/         Windows Host（输入捕获 + 剪贴板客户端）
├─ windows-client/  Windows Client（输入注入 + 剪贴板服务端）
├─ linux/           Linux Client
└─ packaging/       Linux udev 规则
```

所有 Cargo 命令均在项目根目录执行。

## 编译

```powershell
cd D:\githubProject\DeskLink

# Windows Host
cargo build -p desklink-windows --release

# Windows Client
cargo build -p desklink-windows-client --release
```

生成文件：

```text
target\release\desklink-windows.exe
target\release\desklink-windows-client.exe
```

`target\release\desklink-selftest.exe` 是端到端自检工具，部署前可用来验证客户端。

Linux：

```bash
cd ~/DeskLink
cargo build -p desklink-linux --release
```

首次运行前安装 Wayland 剪贴板工具，并配置 uinput 权限规则：

```bash
sudo apt install wl-clipboard xclip
sudo usermod -aG input "$USER"
sudo cp packaging/70-desklink-uinput.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

完成后注销并重新登录 Linux。

## 配置

每台电脑一份 `desklink.toml`（放在 exe 同目录，或用 `DESKLINK_CONFIG` 指定）。两台 `security.token` 必须完全一致。

Windows Host（接键鼠的那台）：

```toml
[network]
target = "192.168.66.2:24801"   # 被控端 IP + 输入端口
ui_bind = "127.0.0.1:24802"
control_port = 24800

[security]
token = "请替换为自己的随机令牌"

[topology]
remote_width = 1920             # 被控端屏幕分辨率
remote_height = 1080
edge = "right"
```

Windows Client（被控的那台）：

```toml
[network]
bind = "0.0.0.0:24801"          # 接收输入
control_bind = "0.0.0.0:24800"  # 剪贴板服务端

[security]
token = "请替换为自己的随机令牌"

[topology]
remote_width = 1920             # 自己这块屏幕的分辨率
remote_height = 1080
```

Linux Client：

```toml
[network]
bind = "0.0.0.0:24801"
control_bind = "0.0.0.0:24800"

[security]
token = "请替换为自己的随机令牌"
```

剪贴板默认配置如下。`max_bytes` 默认是 64 MiB；如果需要同步更大的文本，可在两端同时调大。设置 `enabled = false` 可关闭剪贴板同步。

```toml
[clipboard]
enabled = true
poll_ms = 400
max_bytes = 67108864
```

环境变量 `DESKLINK_TARGET`、`DESKLINK_BIND`、`DESKLINK_TOKEN` 和 `DESKLINK_CONFIG` 的优先级高于配置文件。

`desklink.toml` 包含本地地址和令牌，已加入 `.gitignore`，不会提交到 Git。

## 启动

Windows Host：

```powershell
.\target\release\desklink-windows.exe
```

Windows Client：

```powershell
.\target\release\desklink-windows-client.exe
```

Linux：

```bash
./target/release/desklink-linux
```

启动顺序不限。连接成功后：

- Host 日志：`remote client acknowledged; connection ready`
- Windows Client 日志：`Windows host authorized`，并打印本机虚拟桌面尺寸
- Linux 日志：`Windows host authorized`

剪贴板通道建立后，两端会显示 `clipboard channel connected`。复制纯文本后通常会在 `poll_ms` 内同步到另一端；文件、图片和富文本暂不同步。

## 屏幕布局

Host 启动后，在 Host 的浏览器打开：

```text
http://127.0.0.1:24802
```

页面会显示每块 Windows 显示器的真实矩形。拖动绿色远端屏幕到任意 Windows 屏幕旁边，靠近边缘时会自动吸附。

只有两个屏幕实际贴合，并且边缘存在重叠区间时，才会生成 EdgeLink。布局会立即保存到 Host 本地 `desklink.toml`，重启后继续使用。

**远端方块的宽高必须等于被控端的分辨率**。被控端启动日志里的 `desktop=1920x1080+0+0` 就是真实值；不一致时被控端会打印一条 WARN，光标位移会按比例缩放而不是 1:1。

如果 Host 显示器数量、分辨率、缩放或坐标发生变化，DeskLink 会检测到布局指纹变化，使旧布局失效，并提示重新打开 localhost 页面调整。

## 自动穿越

被控端在线并完成 ACK 后，把鼠标移动到有效 EdgeLink。鼠标在边缘停留 `edge_delay_ms`（默认 80ms）后自动进入远端，不需要输入终端命令。

被控端不读取远端光标位置，而是根据收到的 `dx/dy` 维护逻辑光标。移动到对应返回边缘后，Host 会把光标恢复到正确显示器和对应位置。

紧急恢复快捷键（在 Host 上按）：

```text
Ctrl + Alt + Esc
```

## 防火墙

Ubuntu/Linux：

```bash
sudo ufw allow 24801/udp
sudo ufw allow 24800/tcp
```

Windows（在**被控端**放行入站；以管理员身份运行）：

```powershell
netsh advfirewall firewall add rule name="DeskLink Input" dir=in action=allow protocol=UDP localport=24801
netsh advfirewall firewall add rule name="DeskLink Clipboard" dir=in action=allow protocol=TCP localport=24800
```

直连网线或被系统识别为「未识别的网络」时，网络类别是「公用」，默认会拦截入站，必须显式放行。

## 故障排查

如果 Host 显示 `remote client ACK timed out`：

1. 确认被控端程序正在运行。
2. 确认两端均由当前源码重新编译，协议版本一致。
3. 确认两端 `security.token` 完全一致。
4. 确认 Host 的 `network.target` 是被控端的真实局域网 IP。
5. 确认 UDP 24801 未被防火墙拦截。
6. 注意 PowerShell 环境变量可能覆盖 `desklink.toml`。

Windows Client 不注入输入：

1. 看是否打印 `input injection failed`。`SendInput` 无法注入到更高权限（管理员）窗口，被控端与被控窗口需要相同权限级别。
2. 锁屏/切换用户时注入无效，这是 Windows 的限制。
3. 如果从 DSH 工作区内直接运行 exe，输入注入可能被进程沙箱拦截（`SetCursorPos` 返回失败）；把 exe 复制到普通目录（如 `C:\DeskLink\`）再运行。

查看 Linux 监听状态：

```bash
ss -lunp | grep 24801
ss -ltnp | grep 24800
pgrep -af desklink-linux
```

Windows 查看监听状态：

```powershell
Get-NetUDPEndpoint -LocalPort 24801
Get-NetTCPConnection -LocalPort 24800 -State Listen
```

版本变化参见 [CHANGELOG.md](CHANGELOG.md)。Windows 双机部署步骤见 [docs/windows-to-windows.md](docs/windows-to-windows.md)。

## 来源与许可

本项目基于 [yuanzhechn/DeskLink](https://github.com/yuanzhechn/DeskLink) 0.6.2 修改而来：新增 windows-client（Windows 被控端）、Windows ↔ Windows 文档与自检工具，主机端只调整了与平台无关的日志和布局页文案，协议与端口保持兼容。上游与本仓库均以 MIT 许可发布，见 [LICENSE](LICENSE)。