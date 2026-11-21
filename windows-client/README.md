# DeskLink Windows Client

Windows Client 是「被控端」：接收 Host 发来的输入并注入本机，同时作为剪贴板通道的 TCP 服务端。

- 输入：UDP 24801（`network.bind`），`SendInput` 注入按键/滚轮/按键，光标用 `SetCursorPos` 绝对定位。
- 剪贴板：TCP 24800（`network.control_bind`），Host 主动连接，协议为 `protocol::ControlMessage`。
- 与 Host 相同地处理 `Hello/Heartbeat/Input/EnterRemote/SetState/ReleaseAll`，并回 `Ack`；光标越过返回边缘时回 `EdgeReturn`。

## 运行

```powershell
copy config.example.toml desklink.toml
# 改 [network] bind/control_bind 与 [security] token，token 必须与 Host 一致
.\desklink-windows-client.exe
```

启动日志会打印本机虚拟桌面尺寸，例如：

```text
DeskLink Windows Client listening bind=0.0.0.0:24801 protocol=2 ... desktop=1920x1080+0+0
```

Host 的 `topology.remote_width/remote_height` 必须等于这个尺寸。

## 调试

- `DESKLINK_DRY_RUN=1`：只解析并打印事件，不注入输入，适合在单机上联调。
- `DESKLINK_DRY_RUN=1 desklink-windows-client.exe` + `desklink-selftest.exe`：跑完整链路自检
  （握手、心跳、错误令牌拒绝、进入远端、边缘返回、剪贴板写入）。

## 已知限制

- 左右修饰键在 Host 侧会合并为左侧（`Ctrl/Alt/Shift/Win` 的右侧版本不接受）。
- `PrintScreen`、媒体键等未映射的虚拟键不保证可用。
- 光标用绝对定位注入，依赖原始输入（raw input）的游戏不会看到相对位移。
- 剪贴板只同步纯文本。
- 被控端锁屏时无法注入。
