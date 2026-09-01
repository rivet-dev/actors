#if !FISHNET_THREADED_TICKSMOOTHERS
using FishNet.Object;
using UnityEngine;

namespace FishNet.Component.Transforming.Beta
{
    [System.Serializable]
    public struct MovementSettings
    {
        /// <summary>
        /// True to enable teleport threshold.
        /// </summary>
        [Tooltip("True to enable teleport threshold.")]
        public bool EnableTeleport;
        /// <summary>
        /// How far the object must move between ticks to teleport rather than smooth.
        /// </summary>
        [Tooltip("How far the object must move between ticks to teleport rather than smooth.")]
        [Range(0f, ushort.MaxValue)]
        public float TeleportThreshold;
        /// <summary>
        /// Amount of adaptive interpolation to use. Adaptive interpolation increases interpolation with the local client's latency. Lower values of adaptive interpolation results in smaller interpolation increases.
        /// In most cases adaptive interpolation is only used with prediction where objects might be affected by other moving objects.
        /// </summary>
        [Tooltip("Amount of adaptive interpolation to use. Adaptive interpolation increases interpolation with the local client's latency. Lower values of adaptive interpolation results in smaller interpolation increases. In most cases adaptive interpolation is only used with prediction where objects might be affected by other moving objects.")]
        public AdaptiveInterpolationType AdaptiveInterpolationValue;
        /// <summary>
        /// Number of ticks to smooth over when not using adaptive interpolation.
        /// </summary>
        [Tooltip("Number of ticks to smooth over when not using adaptive interpolation.")]
        public byte InterpolationValue;
        /// <summary>
        /// Properties to smooth. Any value not selected will become offset with every movement.
        /// </summary>
        [Tooltip("Properties to smooth. Any value not selected will become offset with every movement.")]
        public TransformPropertiesFlag SmoothedProperties;
        /// <summary>
        /// True to keep non-smoothed properties at their original localspace every tick. A false value will keep the properties in the same world space as they were before each tick.
        /// </summary>
        [Tooltip("True to keep non-smoothed properties at their original localspace every tick. A false value will keep the properties in the same world space as they were before each tick.")]
        public bool SnapNonSmoothedProperties;

        public MovementSettings(bool unityReallyNeedsToSupportParameterlessInitializersOnStructsAlready)
        {
            EnableTeleport = false;
            TeleportThreshold = 0f;
            AdaptiveInterpolationValue = AdaptiveInterpolationType.Off;
            InterpolationValue = 2;
            SmoothedProperties = TransformPropertiesFlag.Everything;
            SnapNonSmoothedProperties = false;
        }
    }
}
#endif