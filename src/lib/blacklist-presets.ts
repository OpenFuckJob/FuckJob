export interface BlacklistPreset {
  id: string;
  label: string;
  description: string;
  target: "exclude_keywords" | "company_exclude_keywords";
  keywords: string[];
}

/** 面向国内招聘平台高频痛点的反噪黑名单预设包，点击即可增量合并进筛选词列表 */
export const BLACKLIST_PRESETS: BlacklistPreset[] = [
  {
    id: "outsource_companies",
    label: "常见 IT 外包公司",
    description: "过滤大型人力外包与驻场服务商",
    target: "company_exclude_keywords",
    keywords: [
      "软通动力",
      "中软国际",
      "博彦科技",
      "法本信息",
      "文思海辉",
      "东软",
      "润和软件",
      "诚迈科技",
      "外企德科",
      "科锐国际",
      "亿达信息",
      "微创软件",
      "易思博",
      "信必优",
      "神州信息",
    ],
  },
  {
    id: "outsource_titles",
    label: "外包 / 驻场 / OD 词",
    description: "在标题阶段直接跳过外包与派遣类岗位",
    target: "exclude_keywords",
    keywords: ["外包", "驻场", "外派", "劳务派遣", "OD", "人力外包", "项目外包"],
  },
  {
    id: "training_scam",
    label: "招转培 / 培训贷高危词",
    description: "拦截学徒、实训、零基础包教等引流帖",
    target: "exclude_keywords",
    keywords: ["招转培", "岗前实训", "带薪培训", "零基础包教", "学徒", "实训生", "转行培训"],
  },
  {
    id: "sales_disguised",
    label: "电销 / 保险 / 伪文员词",
    description: "拦截伪装成技术或运营岗的电销/保险岗位",
    target: "exclude_keywords",
    keywords: ["电销", "电话销售", "保险代理", "理财顾问", "网销", "邀约客服", "地推", "贷款专员"],
  },
];

/** 增量去重合并筛选词：保留已有词顺序与大小写，按小写去重并忽略空白 */
export function mergeUniqueKeywords(
  existing: string[] | undefined,
  incoming: string[] | undefined,
): string[] {
  const result: string[] = [];
  const seen = new Set<string>();
  for (const raw of [...(existing ?? []), ...(incoming ?? [])]) {
    const trimmed = raw.trim();
    if (!trimmed) continue;
    const key = trimmed.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(trimmed);
  }
  return result;
}
