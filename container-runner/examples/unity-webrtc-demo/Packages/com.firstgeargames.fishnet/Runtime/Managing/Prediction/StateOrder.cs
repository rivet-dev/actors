namespace FishNet.Managing.Predicting
{
    public enum ReplicateStateOrder
    {
        /// <summary>
        /// On clients states are placed in the past and then run when reconciles occur.
        /// This ensures that the local client is running states at the exact same time the server had, resulting in the best possible outcome for prediction accuracy.
        /// To function properly however, clients must reconcile regularly to run past inputs which may cause performance loss on lower-end client devices.
        /// </summary>
        Inserted,
        /// <summary>
        /// On clients states are still placed in the past but rather than wait until a reconcile to run, they are also placed into a queue and run as they are received.
        /// This causes states to initially run out of tick alignment, while correcting during reconciles.
        /// However, due to states no longer depending on reconciles to be run reconciles may be sent less, and clients may run reconciles less, resulting in high performance gain especially among physics-based games.
        /// </summary>
        Appended
    }
}