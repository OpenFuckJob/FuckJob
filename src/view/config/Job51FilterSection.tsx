import { Card, Cascader, Form, Select, Typography, message } from "antd";
import catalog from "../../assets/resource/job51.json";
import { DEFAULT_JOB51_FILTER_CONFIG, type Job51FilterConfig } from "../../types/app-config";

type CatalogNode = { code: string; label: string; children?: CatalogNode[] };
type Option = { value: string; label: string; disableCheckbox: boolean; children?: Option[] };
const pathByCode = new Map<string, string[]>();
function optionsFor(nodes: CatalogNode[], parent: string[] = []): Option[] {
  return nodes.map(node => {
    const path = [...parent, node.code];
    if (!node.children?.length) pathByCode.set(node.code, path);
    return { value: node.code, label: node.label, disableCheckbox: Boolean(node.children?.length),
      ...(node.children?.length ? { children: optionsFor(node.children, path) } : {}) };
  });
}
const functionOptions = optionsFor(catalog.functions);
const selectOptions = (items: {code:string;label:string}[]) => items.map(row => ({value:row.code,label:row.label}));

export default function Job51FilterSection({ value, onChange }: {
  value?: Job51FilterConfig;
  onChange: (next: Job51FilterConfig) => void;
}) {
  const config = { ...DEFAULT_JOB51_FILTER_CONFIG, ...value };
  return <Card size="small" className="mt-4" title="51job 专属筛选">
    <Typography.Paragraph type="secondary">以下选项仅用于前程无忧，跟随当前求职方案保存。</Typography.Paragraph>
    <Form.Item label="51job 月薪范围" extra="可以多选；留空表示全部。">
      <Select aria-label="51job 月薪范围" mode="multiple" allowClear placeholder="全部月薪范围"
        value={config.salary} options={selectOptions(catalog.salary)}
        onChange={salary => onChange({ ...config, salary })} />
    </Form.Item>
    <Form.Item label="51job 工作职能" extra="最多选择 5 个具体职能，支持按名称搜索。">
      <Cascader aria-label="51job 工作职能" multiple allowClear options={functionOptions}
        value={config.functions.map(code => pathByCode.get(code)).filter((path): path is string[] => Boolean(path))}
        placeholder="全部工作职能" maxTagCount="responsive"
        showSearch={{ filter: (input, path) => path.some(node => String(node.label).toLowerCase().includes(input.toLowerCase())) }}
        onChange={paths => {
          const functions = [...new Set(paths.map(path => String(path[path.length - 1])).filter(code => pathByCode.has(code)))];
          if (functions.length > 5) { void message.warning("51job 最多选择 5 个工作职能"); return; }
          onChange({ ...config, functions });
        }} />
    </Form.Item>
    <Form.Item label="51job 公司规模" extra="可以多选；留空表示全部。" className="mb-0!">
      <Select aria-label="51job 公司规模" mode="multiple" allowClear placeholder="全部公司规模"
        value={config.company_size} options={selectOptions(catalog.company_size)}
        onChange={company_size => onChange({ ...config, company_size })} />
    </Form.Item>
  </Card>;
}
