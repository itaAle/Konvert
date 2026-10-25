// Milestone 1 placeholder: verifies the IPC → Rust → bundled FFmpeg chain.
// The real interface arrives in milestone 3.
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface EngineInfo {
  appVersion: string;
  ffmpegVersion: string;
}

interface ErrorPayload {
  code: string;
  details: string | null;
}

export default function App() {
  const [info, setInfo] = useState<EngineInfo | null>(null);
  const [error, setError] = useState<ErrorPayload | null>(null);

  useEffect(() => {
    invoke<EngineInfo>("engine_info").then(setInfo, setError);
  }, []);

  return (
    <main style={{ fontFamily: "system-ui, sans-serif", padding: 32 }}>
      <h1>Konvert</h1>
      {info && (
        <p>
          Versione {info.appVersion} · {info.ffmpegVersion}
        </p>
      )}
      {error && <p role="alert">Errore: {error.code}</p>}
    </main>
  );
}
