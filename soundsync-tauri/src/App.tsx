import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./index.css";

function App() {
  const [url, setUrl] = useState("");
  const [format, setFormat] = useState("mp3");
  const [loading, setLoading] = useState(false);
  const [metadata, setMetadata] = useState<any>(null);
  const [progress, setProgress] = useState(0);
  const [status, setStatus] = useState("Ready to sync");

  useEffect(() => {
    let unlisten: any;
    async function setup() {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen("download-progress", (event: any) => {
        const p = event.payload as any;
        setProgress(Math.round(p.percentage));
        setStatus(`Downloading: ${p.speed} (ETA: ${p.eta})`);
      });
    }
    setup();
    return () => {
      if (unlisten) unlisten.then((f: any) => f());
    };
  }, []);

  const fetchMetadata = async () => {
    if (!url) return;
    try {
      setStatus("Fetching metadata...");
      const data = await invoke("get_metadata", { url });
      setMetadata(data);
      setStatus("Track found");
    } catch (err) {
      console.error(err);
      setStatus("Error fetching metadata");
    }
  };

  const startDownload = async () => {
    if (!url) return;
    try {
      setLoading(true);
      setStatus("Downloading...");
      setProgress(0);
      
      // Default path for now - in a real app we'd use tauri-plugin-dialog
      const downloadPath = "Downloads"; 
      
      const result = await invoke("download_track", { 
        url, 
        format, 
        path: downloadPath 
      });
      
      setStatus("Complete!");
      setProgress(100);
      setLoading(false);
    } catch (err) {
      console.error(err);
      setStatus("Download failed");
      setLoading(false);
    }
  };

  return (
    <div className="app-container">
      <div className="sidebar">
        <div className="logo">
          <span style={{ fontSize: "1.5rem", fontWeight: "bold" }}>SoundSync</span>
        </div>
        <nav style={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
          <div style={{ color: "var(--accent-primary)", fontWeight: "600" }}>Downloader</div>
          <div style={{ color: "var(--text-muted)" }}>Library</div>
          <div style={{ color: "var(--text-muted)" }}>Settings</div>
        </nav>
      </div>

      <main className="main-content">
        <header>
          <h1>Elevate Your Audio</h1>
          <p className="subtitle">High-fidelity downloads from YouTube and SoundCloud.</p>
        </header>

        <section className="glass-card">
          <div className="download-form">
            <div className="input-group">
              <label>PASTE URL</label>
              <div style={{ display: "flex", gap: "1rem" }}>
                <input
                  type="text"
                  placeholder="https://www.youtube.com/watch?v=..."
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  onBlur={fetchMetadata}
                  style={{ flex: 1 }}
                />
              </div>
            </div>

            <div className="controls-row">
              <div className="input-group" style={{ flex: 1 }}>
                <label>FORMAT</label>
                <select value={format} onChange={(e) => setFormat(e.target.value)}>
                  <option value="mp3">MP3 (320kbps)</option>
                  <option value="wav">WAV (Lossless)</option>
                  <option value="flac">FLAC</option>
                  <option value="m4a">M4A</option>
                </select>
              </div>

              <button 
                className="btn-primary" 
                onClick={startDownload}
                disabled={loading}
              >
                {loading ? "SYNCING..." : "START SYNC"}
              </button>
            </div>
          </div>

          {metadata && (
            <div className="metadata-card">
              <img src={metadata.thumbnail} alt="Thumbnail" className="thumbnail" />
              <div className="meta-info">
                <h3>{metadata.title}</h3>
                <p>{metadata.uploader} • {metadata.duration}</p>
              </div>
            </div>
          )}

          {loading && (
            <div className="progress-container">
              <div className="progress-item">
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <label>{status}</label>
                  <label>{progress}%</label>
                </div>
                <div className="progress-bar-bg">
                  <div className="progress-bar-fill" style={{ width: `${progress}%` }}></div>
                </div>
              </div>
            </div>
          )}
          
          {!loading && status !== "Ready to sync" && (
            <p style={{ marginTop: "1rem", color: "var(--accent-primary)", fontSize: "0.9rem" }}>
              {status}
            </p>
          )}
        </section>
      </main>
    </div>
  );
}

export default App;
