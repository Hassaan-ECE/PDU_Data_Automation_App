/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_PDU_SIMULATION_MODE?: string;
  readonly VITE_PDU_SIMULATION_VERSION?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
