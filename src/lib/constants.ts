import conditions from "@/assets/resource/conditions.json";
import industry from "@/assets/resource/industry.simple.json";
import site from "@/assets/resource/site.json";

export interface Option {
  code: number;
  name: string;
}

export const payTypeOptions: Option[] = conditions.payTypeList;
export const experienceOptions: Option[] = conditions.experienceList;
export const salaryOptions: Option[] = conditions.salaryList;
export const stageOptions: Option[] = conditions.stageList;
export const scaleOptions: Option[] = conditions.scaleList;
export const degreeOptions: Option[] = conditions.degreeList;
export const jobTypeOptions: Option[] = conditions.jobTypeList;

// Flatten industry for simple select
export const industryOptions: Option[] = industry.flatMap((cat) =>
  cat.subLevelModelList.map((sub) => ({
    code: sub.code,
    name: `${cat.name} / ${sub.name}`,
  })),
);

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

// Flatten city for simple select (top level and sub level)
export const cityOptions: Option[] = site.flatMap((prov) => {
  const result: Option[] = [];
  if (prov.subLevelModelList) {
    prov.subLevelModelList.forEach((city) => {
      result.push({ code: city.code, name: city.name });
    });
  } else {
    result.push({ code: prov.code, name: prov.name });
  }
  return result;
});

export const cityTreeOptions: TreeOption[] = site.map((prov) => ({
  value: prov.code,
  label: prov.name,
  children: prov.subLevelModelList?.map((city) => ({
    value: city.code,
    label: city.name,
    // Note: Some cities might have further sub-levels in a full site.json,
    // but based on current read, we're handling 2 levels.
  })),
}));
