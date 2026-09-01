using FishNet.Documenting;

namespace FishNet.Managing.Object
{
    [System.Flags]
    internal enum SpawnType : byte
    {
        Unset = 0,
        /// <summary>
        /// Is nested beneath a NetworkBehaviour.
        /// </summary>
        Nested = 1,
        /// <summary>
        /// Is a scene object.
        /// </summary>
        Scene = 2,
        /// <summary>
        /// Instantiate into active scene.
        /// </summary>
        Instantiated = 4,
        /// <summary>
        /// Instantiate into the global scene.
        /// </summary>
        InstantiatedGlobal = 8,
        /// <summary>
        /// Indicates the receiver is the predicted spawner.
        /// </summary>
        IsPredictedSpawner = 16
    }

    [APIExclude]
    internal static class SpawnTypeExtensions
    {
        /// <summary>
        /// Returns if whole contains part.
        /// </summary>
        public static bool FastContains(this SpawnType whole, SpawnType part)
        {
            return (whole & part) == part;
        }
    }
}