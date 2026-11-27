import conditions from "@/assets/resource/conditions.json";
import industry from "@/assets/resource/industry.simple.json";
import site from "@/assets/resource/site.json";

export interface Option {
  code: number;
  name: string;
}

export const experienceOptions: Option[] = conditions.experienceList;
export const salaryOptions: Option[] = conditions.salaryList;
export const stageOptions: Option[] = conditions.stageList;
export const scaleOptions: Option[] = conditions.scaleList;
export const degreeOptions: Option[] = conditions.degreeList;
export const jobTypeOptions: Option[] = conditions.jobTypeList;

export interface TreeOption {
  value: number;
  label: string;
  children?: TreeOption[];
}

export const industryTreeOptions: TreeOption[] = industry.map((cat) => ({
  value: cat.code,
  label: cat.name,
  children: cat.subLevelModelList.map((sub) => ({
    value: sub.code,
    label: sub.name,
  })),
}));

export const nationwideCityCode = 100010000;

export const cityTreeOptions: TreeOption[] = [
  { value: nationwideCityCode, label: "全国" },
  ...site.map((prov) => ({
    value: prov.code,
    label: prov.name,
    children: prov.subLevelModelList?.map((city) => ({
      value: city.code,
      label: city.name,
      // Note: Some cities might have further sub-levels in a full site.json,
      // but based on current read, we're handling 2 levels.
    })),
  })),
];
