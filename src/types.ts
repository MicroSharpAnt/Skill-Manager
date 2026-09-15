export interface Project {
  root: string;
  name: string;
}
export interface Resource {
  path: string;
  name: string;
  description: string;
  kind: "skill" | "rule";
  provider: string;
  enabled: boolean;
  status: string;
  digest: string;
  updatedAt: number | null;
}
export interface Snapshot {
  root: string;
  branch: string;
  vault: string;
  hook: string;
  resources: Resource[];
  pending: boolean;
  warnings: string[];
}
export interface Collection {
  id: number;
  root: string;
  name: string;
  kind: "group" | "profile";
  paths: string[];
  color: string;
}
export interface Change {
  path: string;
  enable: boolean;
}
