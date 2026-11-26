# 提示词优化

本仓负责提示词优化核心、调用协议和独立窗口。主程序、共享控件和通用合同由比格熊主仓维护。deps/host-inputs.lock.json 登记主仓构建输入，改动先归主仓，再从固定 Git 提交重新生成。官方创作 Skill 在 bgxiong-content-skills 维护。

在独立检出运行 python scripts/sync-host-inputs.py --check 和定向测试。开发及发行成品遵守 RELEASE_OUTPUTS.md。
