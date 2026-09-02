import { actor } from "rivetkit";

const myActor = actor({
  options: {
    maxActions: 128,
    maxQueueSize: 1000,
    actionTimeout: 60_000,
    stateSaveInterval: 1_000,
  },
  // ...
});
