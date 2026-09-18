# Hello World

A minimal example demonstrating RivetKit with a real-time counter shared across multiple clients.

## Getting Started

```sh
git clone https://github.com/rivet-dev/rivet.git
cd rivet/examples/hello-world
npm install
npm run dev
```


## Features

- **Actor state management**: Persistent counter state managed by Rivet Actors
- **Real-time updates**: Counter values synchronized across all connected clients via events
- **Multiple actor instances**: Each counter ID creates a separate actor instance
- **React integration**: Uses `@rivetkit/react` for seamless React hooks integration

## Implementation

This example demonstrates the core RivetKit concepts with a simple counter:

- **Actor and Server** ([`src/index.ts`](https://github.com/rivet-dev/rivet/tree/main/examples/hello-world/src/index.ts)): Counter actor with persistent state and broadcast events, plus actor registration and RivetKit server startup
- **React Frontend** ([`frontend/App.tsx`](https://github.com/rivet-dev/rivet/tree/main/examples/hello-world/frontend/App.tsx)): Counter component using `useActor` hook and event subscriptions

## Resources

Read more about [actions](/docs/actors/actions), [state](/docs/actors/state), and [events](/docs/actors/events).

## License

MIT
