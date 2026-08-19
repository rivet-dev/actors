#if !FISHNET_THREADED_COLLIDER_ROLLBACK
using System;
using System.Runtime.CompilerServices;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Component.ColliderRollback
{
    public partial class ColliderRollback
    {
        internal enum BoundingBoxType
        {
            /// <summary>
            /// Disable this feature.
            /// </summary>
            Disabled,
            /// <summary>
            /// Manually specify the dimensions of a bounding box.
            /// </summary>
            Manual
        }

    }
}
#endif