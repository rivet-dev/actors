namespace FishNet.Component.ColliderRollback
{
    /// <summary>
    /// Which physics to apply after rolling back colliders.
    /// </summary>
    [System.Serializable]
    [System.Flags]
    public enum RollbackPhysicsType
    {
        Physics = 1,
        Physics2D = 2
    }
}