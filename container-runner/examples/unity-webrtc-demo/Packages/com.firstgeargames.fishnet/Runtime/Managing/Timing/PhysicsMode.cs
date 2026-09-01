namespace FishNet.Managing.Timing
{
    /// <summary>
    /// How to simulate physics.
    /// </summary>
    public enum PhysicsMode
    {
        /// <summary>
        /// Unity performs physics every FixedUpdate.
        /// </summary>
        Unity = 0,
        /// <summary>
        /// TimeManager performs physics each tick.
        /// </summary>
        TimeManager = 1,
        /// <summary>
        /// Physics will be disabled.
        /// </summary>
        Disabled = 2
    }
}