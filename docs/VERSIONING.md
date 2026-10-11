# 版本管理

独立 Git 仓库，main 的首次提交是复制基线；开发分支使用 codex/ 前缀。
源项目对应关系见 PROVENANCE.md。当前未配置远程仓库，不会未经授权推送。

- 产品版本统一维护 package.json、package-lock.json、Cargo.toml、Cargo.lock 与 tauri.conf.json。
- `npm run version:set -- X.Y.Z` 同步修改，审查后提交；release preflight 校验版本一致及工作区干净。
- Build 为一次构建的数字身份；同一构建须编译与打包共用 INKSAY_BUILD_NUMBER。
- 完整 Git commit、dirty、target 编入二进制，`--build-info` 可无窗口输出；发行打包读取二进制元数据而非猜测当前源码版本。
- ZIP manifest 记录 SHA-256。源码 tag 可按产品版本创建；只在产品发布时建立 release tag，避免把每次本机构建冒充正式发布。
- 维护 lockfile；依赖升级独立提交并运行双平台 CI。当前 CI 已迁移为根目录项目，远程实际运行需配置仓库。
