namespace FishNet.Observing
{
    /// <summary>
    /// How a condition is handled.
    /// This is intentionally not set as flags.
    /// </summary>
    public enum ObserverConditionType : byte
    {
        /// <summary>
        /// Condition is checked only when changed.
        /// </summary>
        Normal = 1,
        /// <summary>
        /// Condition requires checks at regular intervals. The intervals are handled internally.
        /// </summary>
        Timed = 2
    }
}