export type Group = { id: string; name: string; color: string };
export type Repo = { repo: string; reference: string; enabled: boolean };
export type AdminPlan = {
  token: string;
  kind: string;
  summary: string[];
  warnings: string[];
  moves: { from: string; to: string; fingerprint: string }[];
  before: { records: Record<string, unknown> };
  after: { records: Record<string, unknown> };
};
export type AdminOverview = {
  clientDefaults: Record<string, string>;
  config: {
    library: string;
    syncMethod: string;
    clients: Record<string, string>;
    groups: Group[];
    members: Record<string, string>;
    repos: Repo[];
    imported: string[];
  };
  backups: {
    token: string;
    id: string;
    name: string;
    created: number;
    previous: { expected: string[]; source: string };
  }[];
  ccDir: string;
  history: AdminPlan[];
};
