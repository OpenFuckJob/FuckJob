#!/usr/bin/env python3
"""归因步骤1的离线分类器：同字节重发×5 → 判定「恒定 / 有状态 / 样本不足」。

对应 web-reverse-master recipes-2026-08.md §4.1「失败归因」第 1 步：
  - 恒定（同输入恒定响应）≠ 算法错。时间戳缺失/缺前置接口同样会恒拒 → 必须做原版对照(步骤2)。
  - 漂移 / 随量降级 → 服务端有状态计分 → 跳到维度轮换(步骤4)。

本模块只做**离线判定**：调用方负责真正重发（requests / curl_cffi）并把每次的
status + 一个稳定信号（响应体哈希或长度）喂进来。不联网，可被 selftest 覆盖。

注意：本文件在仓库根 scripts/ 与 .claude/skills/web-reverse-master/scripts/
各存一份，必须保持字节一致（两处 selftest 的 attribution case 都会校验）。
"""

from __future__ import annotations

from typing import Sequence, Mapping, Any


MIN_SAMPLES = 5


def classify_resend(samples: Sequence[Mapping[str, Any]]) -> tuple[str, str]:
    """判定同字节重发结果。

    Args:
        samples: 每项含 ``status``（HTTP 状态码）与 ``signal``（响应体哈希/长度等稳定信号）。
                 应为「逐字节相同的请求」重发 >=5 次的记录。

    Returns:
        (verdict, reason)：
          - ``"constant"``  同输入恒定响应 → 算法错误未证实，去做原版对照。
          - ``"stateful"``  响应随重发漂移/降级 → 有状态计分，跳维度轮换。
          - ``"insufficient"`` 样本不足 5，不允许下结论。
    """
    n = len(samples)
    if n < MIN_SAMPLES:
        return ("insufficient", f"需要 >=5 次同字节重发，仅 {n} 次，禁止下归因结论")

    statuses = {s["status"] for s in samples}
    signals = {s.get("signal") for s in samples}

    if len(statuses) == 1 and len(signals) == 1:
        return (
            "constant",
            "同字节恒定响应：算法错误未证实（时间戳/缺前置接口同样恒拒）→ "
            "做原版对照(§4.1 步骤2)，未两证前禁说「被风控 / 算不出」",
        )
    return (
        "stateful",
        "响应随重发漂移/降级：服务端有状态计分 → 跳到维度轮换 IP/账号/设备(§4.1 步骤4)",
    )


if __name__ == "__main__":  # 手动 smoke
    demo_constant = [{"status": 403, "signal": "h0"} for _ in range(5)]
    demo_drift = [{"status": 403, "signal": f"h{i}"} for i in range(5)]
    print(classify_resend(demo_constant))
    print(classify_resend(demo_drift))
    print(classify_resend(demo_constant[:3]))
