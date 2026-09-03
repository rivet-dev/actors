import { actor } from "rivetkit";

const myActor = actor({
  options: {
    maxQueueSize: 1000,
    actionTimeout: 60_000,
    stateSaveInterval: 1_000,
  },
  // ...
});
