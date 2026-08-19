#if FISHNET_THREADED_COLLIDER_ROLLBACK
using System.Runtime.CompilerServices;
using Unity.Burst;
using Unity.Collections;
using Unity.Jobs;
using Unity.Mathematics;
using UnityEngine;
using UnityEngine.Jobs;

namespace FishNet.Component.ColliderRollback
{
    public partial class RollbackManager
    {
	    internal enum DeferredRollbackOrder : byte
	    {
		    PreTick = 0,
		    Tick = 1,
		    PostTick = 2
	    }
	    
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

        internal struct BoundingBoxData
        {
	        public RollbackPhysicsType rollbackPhysicsType;
	        public BoundingBoxType boundingBoxType;
	        public float3 extends;
	        public float3 center;
	        public quaternion localRotation;

	        public BoundingBoxData(RollbackPhysicsType rollbackPhysicsType, BoundingBoxType boundingBoxType,
		        float3 extends, float3 center, quaternion localRotation)
	        {
		        this.rollbackPhysicsType = rollbackPhysicsType;
		        this.boundingBoxType = boundingBoxType;
		        this.extends = extends;
		        this.center = center;
		        this.localRotation = localRotation;
	        }
        }
        
        /// <summary>
        /// A deferred rollback request.
        /// </summary>
        public struct RollbackRequest
        {
	        public int sceneHandle;
	        public float3 origin;
	        public float3 direction;
	        public float distance;
	        public float time;
	        public RollbackPhysicsType rollbackPhysicsType;

	        public RollbackRequest(int sceneHandle, float3 origin, float3 direction, float distance, float time, RollbackPhysicsType rollbackPhysicsType)
	        {
		        this.sceneHandle = sceneHandle;
		        this.origin = origin;
		        this.direction = direction;
		        this.distance = distance;
		        this.time = time;
		        this.rollbackPhysicsType = rollbackPhysicsType;
	        }
        }

    }
}
#endif