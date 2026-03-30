export type SetupStatus = {
  setup_complete: boolean;
  model_ready: boolean;
  llama_server_available: boolean;
  models_dir: string;
  model_path: string | null;
  mmproj_path: string | null;
};

export type ModelDownloadProgress = {
  step: string;
  file_name: string;
  percent: number;
  downloaded_bytes: number;
  total_bytes: number | null;
  speed: string;
  remaining: string;
};

export type ModelDownloadResult = {
  model_path: string;
  mmproj_path: string;
};
