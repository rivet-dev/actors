#if FISHNET_THREADED_COLLIDER_ROLLBACK
using Unity.Mathematics;
using UnityEngine;
using UnityEngine.Jobs;

namespace FishNet.Component.ColliderRollback
{
	/// <summary>
	/// Used to store where colliders are during the snapshot.
	/// </summary>
	public struct ColliderSnapshot
	{
		public ColliderSnapshot(Transform t)
		{
			t.GetPositionAndRotation(out Vector3 pos, out Quaternion rot);
			WorldPosition = pos;
			WorldRotation = rot;
		}
            
		public ColliderSnapshot(TransformAccess ta)
		{
			ta.GetPositionAndRotation(out Vector3 pos, out Quaternion rot);
			WorldPosition = pos;
			WorldRotation = rot;
		}

		public void SetValues(Transform t)
		{
			t.GetPositionAndRotation(out Vector3 pos, out Quaternion rot);
			WorldPosition = pos;
			WorldRotation = rot;
		}
		
		public void SetValues(TransformAccess ta)
		{
			ta.GetPositionAndRotation(out Vector3 pos, out Quaternion rot);
			WorldPosition = pos;
			WorldRotation = rot;
		}
		
		/// <summary>
		/// WorldPosition of transform during snapshot.
		/// </summary>
		public float3 WorldPosition;
		/// <summary>
		/// WorldRotation of transform during snapshot.
		/// </summary>
		public quaternion WorldRotation;
	}
}
#endif