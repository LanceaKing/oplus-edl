# 发布包与运行条件

首版按以下平台制作归档。版本号来自工作区，下面列出 0.1.0 的文件名。

| 验收目标 | 归档 | 桌面入口 | CLI |
| --- | --- | --- | --- |
| Windows 11 x86_64 | `oplus-edl-0.1.0-windows-x86_64.zip` | `oplus-edl-desktop.exe` | `oplus-edl.exe` |
| macOS 14 arm64 | `oplus-edl-0.1.0-macos-arm64.zip` | `Oplus EDL.app` | `oplus-edl` |
| Ubuntu 24.04 x86_64，GNOME X11 和 Wayland | `oplus-edl-0.1.0-linux-x86_64.tar.gz` | `bin/oplus-edl-desktop` | `bin/oplus-edl` |

所有路径相对于解包后的 `oplus-edl/`。包内含 [桌面指南](desktop.md)、[CLI 指南](cli.md)、[产品许可证](../LICENSE)、[第三方声明](../THIRD_PARTY_NOTICES.md)和 `licenses/`。`BUILD.json` 记录版本、Rust 目标、生产特性和 `Cargo.lock` 的 SHA-256。`MANIFEST.sha256` 覆盖归档内的其他所有文件。应用运行无需 Rust 工具链。使用设备时请单独准备对应的加载器、固件和所需签名材料。

## 校验归档

将计算出的 SHA-256 与归档来源提供的校验值比较。应先确认校验值来自可信渠道，再解包或运行程序。

| 平台 | 示例命令 |
| --- | --- |
| Windows | `Get-FileHash -Algorithm SHA256 .\oplus-edl-0.1.0-windows-x86_64.zip` |
| macOS | `shasum -a 256 oplus-edl-0.1.0-macos-arm64.zip` |
| Ubuntu | `sha256sum oplus-edl-0.1.0-linux-x86_64.tar.gz` |

在解包后的 `oplus-edl/` 内，macOS 用 `shasum -a 256 -c MANIFEST.sha256` 校验文件，Ubuntu 用 `sha256sum -c MANIFEST.sha256`。

## Windows 11

用资源管理器将 ZIP 全部解压到当前用户可访问的目录，再打开 `oplus-edl-desktop.exe`。在该目录打开 PowerShell，执行 `.\oplus-edl.exe --version` 检查 CLI。首版使用未签名的可执行文件。若 SmartScreen 显示未知发布者提示，仅在已核实来源、归档校验通过且系统策略提供继续选项时继续。若系统提示缺少 MSVC 运行库，安装 Microsoft 提供的 x64 Visual C++ Redistributable。 [SmartScreen 说明](https://learn.microsoft.com/en-us/windows/security/operating-system-security/virus-and-threat-protection/microsoft-defender-smartscreen/) · [MSVC 运行库](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist)

USB 后端需要目标设备接口绑定适用的 WinUSB 驱动。请在设备管理器确认接口，并使用设备厂商或管理员提供的签名驱动。串口后端使用系统已识别的 COM 端口和适用的串口驱动。 [WinUSB 安装说明](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/winusb-installation)

## macOS 14

用访达解开 ZIP 后打开 `Oplus EDL.app`。在解包目录内执行 `./oplus-edl --version` 检查 CLI。应用包使用无身份的临时签名（ad hoc），CLI 使用链接器生成的临时签名。首版无 Developer ID 签名或 Apple 公证。若 Gatekeeper 阻止启动，在确认来源和 SHA-256 后，可在系统提供该选项时通过「系统设置 → 隐私与安全性 → 仍要打开」为此应用确认一次。 [Apple 的临时签名说明](https://developer.apple.com/documentation/security/seccodesignatureflags/adhoc?language=objc) · [打开应用说明](https://support.apple.com/zh-cn/102445)

USB 配件需要能在当前用户会话中连接，且设备接口可用。Apple 芯片笔记本询问是否允许配件连接时，请解锁并批准你要使用的设备。切换到 EDL 后可能出现新的配件提示。串口后端使用系统已识别且当前用户可访问的 `/dev/cu.*` 端口。 [Apple 的配件连接说明](https://support.apple.com/zh-cn/102282)

## Ubuntu 24.04

在 GNOME 会话中解包并运行。X11 和 Wayland 使用同一归档。

```sh
tar -xzf oplus-edl-0.1.0-linux-x86_64.tar.gz
cd oplus-edl
./bin/oplus-edl --version
./bin/oplus-edl-desktop
```

桌面需要系统的 OpenGL 或 EGL 驱动和当前会话的 X11 或 Wayland 库。文件和目录选择使用 GNOME 的 XDG Desktop Portal，Zenity 作为后备。普通桌面会话可直接运行解包后的程序。 [GNOME Portal](https://packages.ubuntu.com/noble/xdg-desktop-portal-gnome) · [Zenity](https://packages.ubuntu.com/noble/zenity)

将两个二进制文件放入桌面会话 `PATH` 中的用户目录后，可将 `share/applications/oplus-edl-desktop.desktop` 安装到 `~/.local/share/applications/`。该入口使用 `Exec=oplus-edl-desktop` 和 `TryExec=oplus-edl-desktop`。 [Desktop Entry 规范](https://specifications.freedesktop.org/desktop-entry/latest-single/)

USB 后端需要当前用户能访问目标设备节点。权限不足时，由管理员按实际 VID 和 PID 配置限定设备的 udev 权限，例如 Qualcomm EDL 的 `05c6:9008`。串口后端需要可访问所选 `/dev/tty*` 端口，通常由管理员授予相应的 `dialout` 组权限。发布包采用直接解压运行的方式，驱动和系统权限由宿主机管理。

## 验证状态

本机 macOS arm64 已用锁定依赖和显式 `usb`、`serial`、`cloud` 特性离线构建。已解包核验应用信息、临时签名、两个可执行文件的架构和 macOS 14.0 部署目标，并从归档运行 CLI 的版本和帮助命令。Windows 和 Ubuntu 已配置原生构建与归档校验。三平台干净主机上的桌面启动与操作、Ubuntu 两种显示会话以及 USB 在途读取计时仍待实测。
