#![allow(unused)]
//! Project Scaffolding Templates — `hyper init --template <name>`
//!
//! For 100M users, we need instant project creation. Users should be able to:
//!   hyper init --template rust-cli          # Rust CLI project
//!   hyper init --template python-fastapi    # Python FastAPI project
//!   hyper init --template react-vite        # React + Vite + TypeScript
//!   hyper init --template go-service        # Go microservice
//!
//! Strategy: embed templates at compile time (zero external deps).
//! Each template is a function that generates files in the target directory.
//!
//! Template format:
//! - Directory name: <name>/
//! - Files: created via std::fs::write
//! - Post-generation: optionally run cargo init / npm init / go mod init

use anyhow::{Context, Result, bail};
use std::path::Path;

/// All available templates
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    RustCli,
    RustAxumApi,
    PythonFastApi,
    ReactViteTs,
    GoService,
    DenoApi,
}

impl Template {
    /// List all template names
    pub fn all_names() -> Vec<&'static str> {
        vec![
            "rust-cli",
            "rust-axum-api",
            "python-fastapi",
            "react-vite-ts",
            "go-service",
            "deno-api",
        ]
    }

    /// Parse from name string
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "rust-cli" | "rust_cli" | "cli" | "rust" => Some(Template::RustCli),
            "rust-axum" | "rust-axum-api" | "axum" | "rust-api" => Some(Template::RustAxumApi),
            "python-fastapi" | "fastapi" | "python-api" | "python" => Some(Template::PythonFastApi),
            "react-vite" | "react-vite-ts" | "react" | "vite" | "frontend" => {
                Some(Template::ReactViteTs)
            }
            "go-service" | "go" | "go-api" => Some(Template::GoService),
            "deno-api" | "deno" => Some(Template::DenoApi),
            _ => None,
        }
    }

    /// Human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            Template::RustCli => "Rust CLI application with clap + anyhow + tracing",
            Template::RustAxumApi => "Rust REST API with Axum + sqlx + tokio",
            Template::PythonFastApi => "Python FastAPI backend with SQLAlchemy + Alembic",
            Template::ReactViteTs => "React + TypeScript + Vite + Tailwind CSS",
            Template::GoService => "Go HTTP service with chi router + sqlc",
            Template::DenoApi => "Deno REST API with Hono + Drizzle ORM",
        }
    }

    /// Generate the template in the target directory
    pub fn generate(&self, target_dir: &Path) -> Result<()> {
        // Create directory if it doesn't exist
        std::fs::create_dir_all(target_dir)
            .context("Failed to create target directory")?;

        // Check if directory is non-empty (protect against overwriting)
        let entries: Vec<_> = std::fs::read_dir(target_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).collect())
            .unwrap_or_default();

        if !entries.is_empty() {
            bail!(
                "Directory '{}' is not empty. Use an empty directory for scaffolding.",
                target_dir.display()
            );
        }

        match self {
            Template::RustCli => self.gen_rust_cli(target_dir)?,
            Template::RustAxumApi => self.gen_rust_axum(target_dir)?,
            Template::PythonFastApi => self.gen_python_fastapi(target_dir)?,
            Template::ReactViteTs => self.gen_react_vite_ts(target_dir)?,
            Template::GoService => self.gen_go_service(target_dir)?,
            Template::DenoApi => self.gen_deno_api(target_dir)?,
        }

        println!("✅ Generated {} in {}", self.description(), target_dir.display());
        Ok(())
    }

    // ─── Rust CLI Template ──────────────────────────────────────
    fn gen_rust_cli(&self, dir: &Path) -> Result<()> {
        let name = dir.file_name().unwrap().to_string_lossy();

        // Cargo.toml
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"
description = "A Rust CLI application"

[dependencies]
clap = {{ version = "4.5", features = ["derive"] }}
anyhow = "1.0"
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}
tokio = {{ version = "1.40", features = ["full"] }}
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
"#,
                name
            ),
        )?;

        // .gitignore
        std::fs::write(dir.join(".gitignore"), "/target\n**/*.rs.bk\n.DS_Store\n")?;

        // src/main.rs
        std::fs::create_dir_all(dir.join("src"))?;
        std::fs::write(
            dir.join("src").join("main.rs"),
            format!(
                r#"//! {name} - A Rust CLI application

use anyhow::Result;
use clap::Parser;
use tracing::info;

/// CLI application
#[derive(Parser, Debug)]
#[command(name = "{name}", version, about)]
struct Cli {{
    /// Name to greet
    #[arg(short, long, default_value = "World")]
    name: String,
}}

#[tokio::main]
async fn main() -> Result<()> {{
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    info!("Starting {name}...");
    println!("Hello, {{}}!", cli.name);
    Ok(())
}}
"#,
                name = name
            ),
        )?;

        // .hyperrules
        std::fs::write(
            dir.join(".hyperrules"),
            "## Rules for HyperAgent\n\
             - Use anyhow::Result for all error returns\n\
             - Use tracing for logging (not println)\n\
             - Add #[tokio::test] for async tests\n",
        )?;

        // AGENTS.md
        std::fs::write(
            dir.join("AGENTS.md"),
            format!(
                "# {}\n\n## Build\n```bash\ncargo build --release\ncargo check\ncargo test\n```\n\n## Code Style\n- Rust 2021 edition\n- anyhow for errors\n- tracing for logging\n",
                name
            ),
        )?;

        println!("   📦 Run: cd {} && cargo build", dir.display());
        Ok(())
    }

    // ─── Rust Axum API Template ─────────────────────────────────
    fn gen_rust_axum(&self, dir: &Path) -> Result<()> {
        let name = dir.file_name().unwrap().to_string_lossy();

        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = "0.7"
tokio = {{ version = "1.40", features = ["full"] }}
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
tower-http = {{ version = "0.5", features = ["cors", "trace"] }}
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}
anyhow = "1.0"
sqlx = {{ version = "0.8", features = ["runtime-tokio", "sqlite", "postgres"] }}
uuid = {{ version = "1.10", features = ["v4"] }}
chrono = {{ version = "0.4", features = ["serde"] }}
"#,
                name
            ),
        )?;

        std::fs::create_dir_all(dir.join("src"))?;
        std::fs::write(
            dir.join("src").join("main.rs"),
            format!(
                r#"//! {name} - REST API with Axum

use axum::{{
    Router,
    routing::get,
    Json,
}};
use serde::Serialize;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;

#[derive(Serialize)]
struct HealthResponse {{
    status: &'static str,
    version: &'static str,
}}

async fn health() -> Json<HealthResponse> {{
    Json(HealthResponse {{
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    }})
}}

#[tokio::main]
async fn main() -> anyhow::Result<()> {{
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let app = Router::new()
        .route("/health", get(health))
        .layer(CorsLayer::permissive());

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("{name} listening on {{}}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}}
"#,
                name = name
            ),
        )?;

        std::fs::write(dir.join(".gitignore"), "/target\n.env\n*.db\n")?;
        std::fs::write(dir.join(".hyperrules"), "- Use axum 0.7 patterns\n- Return Result<Json<T>, AppError>\n")?;
        println!("   📦 Run: cd {} && cargo run", dir.display());
        Ok(())
    }

    // ─── Python FastAPI Template ────────────────────────────────
    fn gen_python_fastapi(&self, dir: &Path) -> Result<()> {
        let name = dir.file_name().unwrap().to_string_lossy();

        std::fs::write(
            dir.join("requirements.txt"),
            "fastapi==0.115.0\nuvicorn[standard]==0.30.0\nsqlalchemy==2.0\nalembic==1.13\npydantic==2.9\npython-dotenv==1.0\n",
        )?;

        std::fs::write(
            dir.join("main.py"),
            r#"""{}" - FastAPI Application

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

app = FastAPI(title="{}", version="0.1.0")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.get("/health")
async def health():
    return {"status": "ok", "version": "0.1.0"}


@app.get("/")
async def root():
    return {"message": "Hello from {}"}


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)
"#.replace("{}", name.as_ref()),
        )?;

        std::fs::write(dir.join(".gitignore"), "__pycache__/\n*.pyc\n.env\nvenv/\n")?;
        println!("   📦 Run: cd {} && pip install -r requirements.txt && python main.py", dir.display());
        Ok(())
    }

    // ─── React + Vite + TypeScript ──────────────────────────────
    fn gen_react_vite_ts(&self, dir: &Path) -> Result<()> {
        std::fs::write(
            dir.join("package.json"),
            r#"{
  "name": "react-app",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "preview": "vite preview",
    "lint": "eslint ."
  },
  "dependencies": {
    "react": "^19.0.0",
    "react-dom": "^19.0.0"
  },
  "devDependencies": {
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^4.3.0",
    "typescript": "~5.7.0",
    "vite": "^6.0.0",
    "tailwindcss": "^4.0.0",
    "@tailwindcss/vite": "^4.0.0"
  }
}
"#,
        )?;

        std::fs::write(
            dir.join("tsconfig.json"),
            r#"{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "isolatedModules": true,
    "moduleDetection": "force",
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "forceConsistentCasingInFileNames": true
  },
  "include": ["src"]
}
"#,
        )?;

        std::fs::write(
            dir.join("vite.config.ts"),
            r#"import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
})
"#,
        )?;

        std::fs::create_dir_all(dir.join("src"))?;
        std::fs::write(
            dir.join("src").join("main.tsx"),
            r#"import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import './index.css'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
"#,
        )?;

        std::fs::write(
            dir.join("src").join("App.tsx"),
            r#"export default function App() {
  return (
    <div className="min-h-screen bg-gray-900 text-white flex items-center justify-center">
      <div className="text-center">
        <h1 className="text-4xl font-bold mb-4">⚡ React + Vite + TS</h1>
        <p className="text-gray-400">Edit src/App.tsx to get started</p>
      </div>
    </div>
  )
}
"#,
        )?;

        std::fs::write(
            dir.join("src").join("index.css"),
            "@import \"tailwindcss\";\n\nbody { margin: 0; font-family: system-ui, sans-serif; }\n",
        )?;

        std::fs::write(
            dir.join("index.html"),
            r#"<!doctype html>
<html lang="en">
  <head><meta charset="UTF-8" /><meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>React App</title>
  </head>
  <body><div id="root"></div><script type="module" src="/src/main.tsx"></script></body>
</html>
"#,
        )?;

        std::fs::write(dir.join(".gitignore"), "node_modules/\ndist/\n.env\n")?;
        println!("   📦 Run: cd {} && npm install && npm run dev", dir.display());
        Ok(())
    }

    // ─── Go Service Template ────────────────────────────────────
    fn gen_go_service(&self, dir: &Path) -> Result<()> {
        let name = dir.file_name().unwrap().to_string_lossy();

        std::fs::write(
            dir.join("go.mod"),
            format!("module {}\n\ngo 1.23\n", name),
        )?;

        std::fs::write(
            dir.join("main.go"),
            format!(
                r#"// {} - Go HTTP Service

package main

import (
	"encoding/json"
	"log"
	"net/http"
	"os"
)

type HealthResponse struct {{
	Status  string `json:"status"`
	Version string `json:"version"`
}}

func healthHandler(w http.ResponseWriter, r *http.Request) {{
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(HealthResponse{{
		Status:  "ok",
		Version: "0.1.0",
	}})
}}

func main() {{
	port := os.Getenv("PORT")
	if port == "" {{
		port = "8080"
	}}

	http.HandleFunc("/health", healthHandler)
	log.Printf("{} listening on :%s", port)
	log.Fatal(http.ListenAndServe(":"+port, nil))
}}
"#,
                name, name
            ),
        )?;

        std::fs::write(dir.join(".gitignore"), "*.exe\n*.exe~\n*.dll\n*.so\n*.dylib\n")?;
        println!("   📦 Run: cd {} && go run .", dir.display());
        Ok(())
    }

    // ─── Deno API Template ──────────────────────────────────────
    fn gen_deno_api(&self, dir: &Path) -> Result<()> {
        std::fs::write(
            dir.join("deno.json"),
            r#"{
  "tasks": {
    "dev": "deno run --allow-net --allow-env --watch main.ts",
    "start": "deno run --allow-net --allow-env main.ts"
  },
  "imports": {
    "hono": "jsr:@hono/hono@^4.0.0"
  }
}
"#,
        )?;

        std::fs::write(
            dir.join("main.ts"),
            r#"import { Hono } from 'hono'
import { cors } from 'hono/cors'

const app = new Hono()

app.use('*', cors())

app.get('/health', (c) => c.json({ status: 'ok', version: '0.1.0' }))

app.get('/', (c) => c.json({ message: 'Hello from Deno API' }))

Deno.serve({ port: 8000 }, app.fetch)
console.log('🚀 Server running on http://localhost:8000')
"#,
        )?;

        std::fs::write(dir.join(".gitignore"), ".env\n")?;
        println!("   📦 Run: cd {} && deno task dev", dir.display());
        Ok(())
    }
}

/// Print available templates (for `hyper init --list-templates`)
pub fn list_templates() {
    println!();
    println!("  Available templates:");
    println!();
    for name in Template::all_names() {
        if let Some(t) = Template::from_name(name) {
            println!("    \x1b[33m{:20}\x1b[0m  {}", name, t.description());
        }
    }
    println!();
    println!("  Usage: \x1b[33mhyper init --template <name>\x1b[0m");
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_parsing() {
        assert_eq!(Template::from_name("rust-cli"), Some(Template::RustCli));
        assert_eq!(Template::from_name("fastapi"), Some(Template::PythonFastApi));
        assert_eq!(Template::from_name("react"), Some(Template::ReactViteTs));
        assert_eq!(Template::from_name("go"), Some(Template::GoService));
        assert_eq!(Template::from_name("deno"), Some(Template::DenoApi));
        assert_eq!(Template::from_name("nonexistent"), None);
    }

    #[test]
    fn test_all_names_exist() {
        for name in Template::all_names() {
            assert!(
                Template::from_name(name).is_some(),
                "Template name '{}' should parse",
                name
            );
        }
    }

    #[test]
    fn test_generate_to_temp() {
        let tmp = tempfile::tempdir().unwrap();
        let subdir = tmp.path().join("my-cli");

        let template = Template::RustCli;
        let result = template.generate(&subdir);
        assert!(result.is_ok(), "Generate failed: {:?}", result.err());

        assert!(subdir.join("Cargo.toml").exists());
        assert!(subdir.join("src/main.rs").exists());
        assert!(subdir.join(".gitignore").exists());
    }

    #[test]
    fn test_generate_react_template() {
        let tmp = tempfile::tempdir().unwrap();
        let subdir = tmp.path().join("my-app");

        let result = Template::ReactViteTs.generate(&subdir);
        assert!(result.is_ok(), "Generate failed: {:?}", result.err());

        assert!(subdir.join("package.json").exists());
        assert!(subdir.join("tsconfig.json").exists());
        assert!(subdir.join("vite.config.ts").exists());
        assert!(subdir.join("src/main.tsx").exists());
        assert!(subdir.join("src/App.tsx").exists());
        assert!(subdir.join("index.html").exists());
    }
}
