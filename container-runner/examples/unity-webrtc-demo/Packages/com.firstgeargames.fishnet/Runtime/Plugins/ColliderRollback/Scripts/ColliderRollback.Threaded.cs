#if FISHNET_THREADED_COLLIDER_ROLLBACK
using System.Collections.Generic;
using FishNet.Object;
using GameKit.Dependencies.Utilities;
using Unity.Mathematics;
using UnityEngine;

namespace FishNet.Component.ColliderRollback
{
    public class ColliderRollback : NetworkBehaviour
    {
        #region Serialized.
#pragma warning disable CS0414
        /// <summary>
        /// How to configure the bounding box check.
        /// </summary>
        [Tooltip("How to configure the bounding box check.")]
        [SerializeField]
        private RollbackManager.BoundingBoxType _boundingBox = RollbackManager.BoundingBoxType.Disabled;
        /// <summary>
        /// Physics type to generate a bounding box for.
        /// </summary>
        [Tooltip("Physics type to generate a bounding box for.")]
        [SerializeField]
        private RollbackPhysicsType _physicsType = RollbackPhysicsType.Physics;
        /// <summary>
        /// Size for the bounding box. This is only used when BoundingBox is set to Manual.
        /// </summary>
        [Tooltip("Size for the bounding box.. This is only used when BoundingBox is set to Manual.")]
        [SerializeField]
        private Vector3 _boundingBoxSize = new(3f, 3f, 3f);
        /// <summary>
        /// Center for the bounding box. This is only used when BoundingBox is set to Manual.
        /// </summary>
        [Tooltip("Center for the bounding box.. This is only used when BoundingBox is set to Manual.")]
        [SerializeField]
        private Vector3 _boundingBoxCenter = new(0f, 0f, 0f);
        /// <summary>
        /// Local Rotation for the bounding box. This is only used when BoundingBox is set to Manual.
        /// </summary>
        [Tooltip("Center for the bounding box.. This is only used when BoundingBox is set to Manual.")]
        [SerializeField]
        private Quaternion _boundingBoxLocalRotation = Quaternion.identity;
        /// <summary>
        /// Objects holding colliders which can rollback.
        /// </summary>
        [Tooltip("Objects holding colliders which can rollback.")]
        [SerializeField]
        private GameObject[] _colliderParents = new GameObject[0];
#pragma warning restore CS0414
        #endregion

    }
}
#endif