Verify the basic HTML app works at {{URL}}.

Use shell commands only. Do not use browser tools.

From the current working directory, start the app yourself if it is not already running. Prefer the declared project script or obvious static-server command for the project.

## Steps

1. Fetch `{{URL}}` with `/usr/bin/curl -fsS`
2. Confirm the HTML contains `<h1>Hello from Eval</h1>`
3. Confirm the HTML contains `basic smoke test`

## Pass criteria

- The page loads successfully
- The HTML contains `<h1>Hello from Eval</h1>`
- The HTML contains `basic smoke test`
