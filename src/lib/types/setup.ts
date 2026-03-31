export type SetupStatus = {
  setup_complete: boolean;
  engine_ready: boolean;
  node_available: boolean;
  copilot_server_available: boolean;
  edge_debugging_ready: boolean;
  edge_debugging_url: string;
};
