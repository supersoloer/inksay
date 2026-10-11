# 数据与恢复契约

## 位置

macOS 数据：`~/Library/Application Support/com.supersoloer.inksay.macos/`
Windows 数据：`%APPDATA%\com.supersoloer.inksay.windows\`
使用 Tauri app_data_dir 解析，不依赖仓库位置。缓存使用平台 app_cache_dir。

业务文件：config.json、history.json、dictionary.json、term-memory.json、replacement-rules.json、statistics.json。
录音 recordings/<session UUID>.wav（处理中 source.wav），模型 models/。关闭录音保留时只使用缓存目录 inflight-recordings，启动时清理该临时目录。
运行日志 inksay.log / inksay.log.1；恢复记录 recovery.log。静音恢复锁为瞬态运行状态，不做历史备份，恢复成功即移除。
密钥：系统凭据存储，以应用标识加 `.refinement` / `.transcription` 隔离。云端密钥默认绑定请求地址；百炼官方公共域名与 MaaS 网关使用相同 HTTPS API 路径时可复用。自定义服务域名或接口路径变化仍需明确保存密钥，首页状态与实际请求采用同一判断。

## 格式版本

所有业务 JSON 文件统一封装为 `{ "schemaVersion": 1, "data": ... }`。
IPC 返回的业务结构不受磁盘封装影响。v1 是新应用初始版本，不导入旧应用的无版本文件。
未来升级必须在 persistence 解码入口新增明确迁移，并测试旧版本输入及重启幂等性。未知的较高版本拒绝打开，绝不从旧备份降级覆盖。

## 写入与恢复

1. 序列化完成后写同目录随机临时文件；Unix 权限 0600。
2. 覆盖前保留一份结构有效的上一版 `.json.bak`。
3. 文件 sync_all 后同卷 rename；Unix 同步父目录。临时写失败清理临时文件。
4. 文件缺失且无备份视作尚未创建。损坏/缺失且备份有效则恢复；损坏原件另存 `.json.corrupt-UUID`。
5. 主文件和备份都无效则明确报错，启动时弹出读取失败提示，不创建空数据替代。
6. 读取权限等 I/O 错误直接报告，不当作内容损坏自动修复。

备份只保证单文件最近一次有效写入，不是跨文件事务或用户历史归档。统计在启动时按现存历史更新；历史被删除后累计统计仍保留。磁盘完全损坏、备份同时损坏无法自动恢复。
历史清空/删除不代表立即擦除恢复备份；若需彻底移除本机内容，应退出应用后移除整个独立数据目录及系统凭据。隔离文件保留给人工排查，不主动丢弃用户数据。
