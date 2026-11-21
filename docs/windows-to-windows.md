# Windows ↔ Windows 部署指南

目标：用一套键鼠控制两台 Windows，剪贴板互通。下面是直连网线（也可换成同一路由器）的完整步骤。

## 0. 角色

| 角色 | 程序 | 装在哪 |
|---|---|---|
| 主机 Host | `desklink-windows.exe` | 接键盘鼠标的那台 |
| 被控端 Client | `desklink-windows-client.exe` | 另一台 |

输入方向只有 Host → Client 一个方向。想反过来控制，把两台角色对调即可。

## 1. 网络

### 方案 A：网线直连（推荐设静态 IP）

两台直连时，Windows 通常各自拿到 `169.254.x.x`（APIPA）。能用，但换网线/重启后地址可能变，建议固定：

在被控端（管理员 PowerShell，`以太网` 换成实际网卡名）：

```powershell
netsh interface ipv4 show interfaces
netsh interface ipv4 set address name="以太网" static 192.168.66.2 255.255.255.0
```

主机同样操作，地址用 `192.168.66.1`。

验证（两台互 ping）：

```powershell
ping 192.168.66.1
```

回退成自动获取：

```powershell
netsh interface ipv4 set address name="以太网" source=dhcp
```

### 方案 B：同一台路由器 / 交换机

直接用各自现有的局域网地址，`ipconfig` 里对应那块网卡的 IPv4 即可。

## 2. 防火墙（在被控端放行）

直连时那块网卡常被识别为「未识别的网络」（公用网络），默认拦截入站。管理员 PowerShell：

```powershell
netsh advfirewall firewall add rule name="DeskLink Input" dir=in action=allow protocol=UDP localport=24801
netsh advfirewall firewall add rule name="DeskLink Clipboard" dir=in action=allow protocol=TCP localport=24800
```

检查端口是否在监听：

```powershell
Get-NetUDPEndpoint -LocalPort 24801
Get-NetTCPConnection -LocalPort 24800 -State Listen
```

## 3. 放文件

两台各建一个普通目录，例如 `C:\DeskLink\`：

```text
主机   C:\DeskLink\desklink-windows.exe + desklink.toml
被控端 C:\DeskLink\desklink-windows-client.exe + desklink.toml
```

> 不要把 exe 放在 DSH 工作区目录里运行：从工作区启动的进程会被进程沙箱限制，`SetCursorPos`/`SendInput` 会被静默拦截。放到 `C:\DeskLink\` 这类普通目录即可。

## 4. 配置

两台各一份 `desklink.toml`，**`security.token` 必须完全一致**。

主机 `C:\DeskLink\desklink.toml`：

```toml
[network]
target = "192.168.66.2:24801"
ui_bind = "127.0.0.1:24802"
control_port = 24800

[security]
token = "f3633c1bfa275a3d2d7512a2"

[topology]
remote_width = 1920
remote_height = 1080
edge = "right"
```

被控端 `C:\DeskLink\desklink.toml`：

```toml
[network]
bind = "0.0.0.0:24801"
control_bind = "0.0.0.0:24800"

[security]
token = "f3633c1bfa275a3d2d7512a2"

[topology]
remote_width = 1920
remote_height = 1080
```

`topology.remote_width/remote_height` 填**被控端屏幕分辨率**。被控端启动时会打印真实值：

```text
DeskLink Windows Client listening bind=0.0.0.0:24801 ... desktop=1920x1080+0+0
```

`1920x1080+0+0` 就是宽、高、虚拟桌面原点。不一致时被控端会打印 WARN，光标按比例缩放而不是 1:1。

## 5. 启动

先启被控端，再启主机（反过来也行）。

被控端日志出现：

```text
Windows host authorized
```

主机日志出现：

```text
remote client acknowledged; connection ready
```

两端随后会出现 `clipboard channel connected`，剪贴板通道就绪。

## 6. 屏幕布局（关键一步）

在**主机**浏览器打开：

```text
http://127.0.0.1:24802
```

页面里每块灰色矩形是本机显示器，绿色矩形代表被控端：

1. 把绿块拖到主机屏幕旁边（例如右侧贴边会自动吸附）。
2. 绿块宽高改成被控端真实分辨率（页面会保存到 `desklink.toml`）。
3. 只有贴边且边缘有重叠时才生成 EdgeLink，才会生效。

## 7. 使用

- 鼠标推到主机屏幕的对应边缘，停留约 80ms 后进入被控端。
- 在被控端把鼠标推到同一侧边缘并继续往外推，控制权回到主机。
- 主机上按 `Ctrl + Alt + Esc` 随时强制收回本地输入。
- 复制纯文本会自动同步到另一端（默认 400ms 轮询）。

## 8. 部署前自检

在被控端用 dry-run 模式起客户端（不注入输入，安全）：

```powershell
$env:DESKLINK_DRY_RUN = "1"
.\desklink-windows-client.exe
```

另开一个窗口（token 换成你的）：

```powershell
.\desklink-selftest.exe 127.0.0.1:24801 127.0.0.1:24800 f3633c1bfa275a3d2d7512a2
```

全部 `[PASS]` 说明握手、心跳、拒绝错误令牌、进入/返回边缘、剪贴板写回都正常。

## 9. 常见问题

| 现象 | 排查 |
|---|---|
| 主机 `remote client ACK timed out` | token 不一致 / `target` IP 不对 / 被控端没在跑 / UDP 24801 被防火墙拦 |
| 日志正常，鼠标进不去 | 布局页绿块没贴边、没重叠，或宽高不等于被控端分辨率 |
| 进去了但光标不动 | 被控端锁屏；或 exe 从 DSH 工作区启动被沙箱限制；或 `SetCursorPos` 失败（日志 `failed to move the cursor`） |
| 键盘没反应 | 目标窗口以管理员权限运行（UIPI 限制注入）；被控端与被控窗口权限级别需一致 |
| 剪贴板不同步 | 两端 `clipboard.enabled = true`、TCP 24800 通、内容为纯文本 |
| 提示 `clipboard token mismatch` | 两份配置的 token 不一样 |

## 10. 已知限制

- 左右 `Ctrl/Alt/Shift/Win` 在主机侧合并为左侧键。
- `PrintScreen`、媒体键等未映射键不保证可用。
- 光标是绝对定位注入，依赖原始输入（raw input）的游戏收不到相对位移。
- 只同步纯文本剪贴板。
- 被控端锁屏时无法注入。
