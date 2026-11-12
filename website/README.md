# Lovelace website

The site is static HTML, CSS, JavaScript, and images. The Dockerfile serves it with unprivileged Nginx on port `8080`. The desktop app, models, and recordings run on the user's computer.

## Deploy on Coolify

Connect the Git repository as an application, select the branch that contains this folder, and use these settings.

| Setting | Value |
| --- | --- |
| Build pack | Dockerfile |
| Base Directory | `/website` |
| Dockerfile Location | `/Dockerfile` |
| Ports Exposes | `8080` |
| Domain | The HTTPS domain for the website |
| Health check | HTTP GET `/` on port `8080`, expecting `200` |

Deploy and open the configured domain. The application needs no environment variables or volumes. See the [Coolify Dockerfile guide](https://coolify.io/docs/applications/builds/dockerfile).

## Run the container locally

From the repository root:

```sh
docker build -t lovelace-website ./website
docker run --rm -p 127.0.0.1:8080:8080 lovelace-website
```

Open <http://localhost:8080/>. For a quick preview without Docker, open `index.html` directly.

## Set the repository and download links

Edit [repository.js](repository.js). Set `source` to the public Git repository URL. Set `windows` and `linux` to the public asset URLs for `lovelace-windows-x64.zip` and `lovelace-linux-x64.tar.gz`. Each empty value keeps its own “coming soon” message. Rebuild and redeploy after editing.

The [Build binaries workflow](../.github/workflows/build.yml) creates both archives as GitHub Actions artifacts. Attach the tested archives to a GitHub Release to give the site stable download URLs. Add the project's chosen license to the repository so readers can find the terms when they inspect the source.

See the [project README](../README.md) for the desktop application and CLI.
