# Durable Objects Starter

[![Deploy to Cloudflare](https://deploy.workers.cloudflare.com/button)](https://deploy.workers.cloudflare.com/?url=https://github.com/cloudflare/templates/tree/main/hello-world-do-template)

<!-- dash-content-start -->

This is a [Durable Object](https://developers.cloudflare.com/durable-objects/) starter template. It comes with a `sayHello` method that returns `Hello World!`.

<!-- dash-content-end -->

Outside of this repo, you can start a new project with this template using [C3](https://developers.cloudflare.com/pages/get-started/c3/) (the `create-cloudflare` CLI):

```bash
npm create cloudflare@latest -- --template=cloudflare/templates/hello-world-do-template
```

## Getting Started

First, run:

```bash
npm install
# or
yarn install
# or
pnpm install
# or
bun install
```

Then run the development server (using the package manager of your choice):

```bash
npm run dev
```

Open [http://localhost:8787](http://localhost:8787) with your browser to see the result.

You can start editing the project by modifying `src/index.ts`.

## Deploying To Production

| Command             | Action                                |
| :------------------ | :------------------------------------ |
| `npm run deploy`    | Deploy your application to Cloudflare |
| `npm wrangler tail` | View real-time logs for all Workers   |

## Learn More

To learn more about Next.js, take a look at the following resources:

- [Durable Objects](https://developers.cloudflare.com/durable-objects/) - learn about Durable Objects

Your feedback and contributions are welcome!
