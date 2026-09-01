using System;
using FishNet.Documenting;
using FishNet.Object;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace FishNet.Utility.Extension
{
    [APIExclude]
    public static class TransformFN
    {
        /// <summary>
        /// Sets values of TransformProperties to a transforms world properties.
        /// </summary>
        public static TransformProperties GetWorldProperties(this Transform t)
        {
            TransformProperties tp = new(t.position, t.rotation, t.localScale);
            return tp;
        }

        /// <summary>
        /// Sets values of TransformProperties to a transforms world properties.
        /// </summary>
        public static TransformProperties GetWorldProperties(this Transform t, TransformProperties offset)
        {
            TransformProperties tp = new(t.position, t.rotation, t.localScale);
            tp.Add(offset);
            return tp;
        }

        /// <summary>
        /// Sets values of TransformProperties to a transforms world properties.
        /// </summary>
        public static TransformPropertiesCls GetWorldPropertiesCls(this Transform t)
        {
            TransformPropertiesCls tp = new(t.position, t.rotation, t.localScale);
            return tp;
        }

        /// <summary>
        /// Sets values of TransformProperties to a transforms world properties.
        /// </summary>
        public static TransformProperties GetLocalProperties(this Transform t)
        {
            TransformProperties tp = new(t.localPosition, t.localRotation, t.localScale);
            return tp;
        }

        /// <summary>
        /// Sets values of TransformProperties to a transforms world properties.
        /// </summary>
        public static TransformPropertiesCls GetLocalPropertiesCls(this Transform t)
        {
            TransformPropertiesCls tp = new(t.localPosition, t.localRotation, t.localScale);
            return tp;
        }

        /// <summary>
        /// Sets values of TransformPropertiesCls to a transforms world properties.
        /// </summary>
        [Obsolete("Use TransformPropertiesExtensions.SetWorldProperties.")]
        public static void SetWorldProperties(this TransformPropertiesCls tp, Transform t) => TransformPropertiesExtensions.SetWorldProperties(tp, t);

        /// <summary>
        /// Gets the offset values by subtracting this from target.
        /// </summary>
        /// <param name = "pos">Position offset result.</param>
        /// <param name = "rot">Rotation offset result.</param>
        public static void SetTransformOffsets(this Transform t, Transform target, ref Vector3 pos, ref Quaternion rot)
        {
            if (target == null)
                return;
            pos = target.position - t.position;
            rot = target.rotation * Quaternion.Inverse(t.rotation);
        }

        /// <summary>
        /// Gets the offset values by subtracting this from target.
        /// </summary>
        /// <param name = "zeroScale">True to set scale to Vector3.zero.</param>
        public static TransformProperties GetTransformOffsets(this Transform t, Transform target)
        {
            if (target == null)
                return default;

            return new(target.position - t.position, target.rotation * Quaternion.Inverse(t.rotation), target.localScale - t.localScale);
        }

        /// <summary>
        /// Sets a transform to local properties.
        /// </summary>
        public static void SetLocalProperties(this Transform t, TransformPropertiesCls tp)
        {
            t.localPosition = tp.Position;
            t.localRotation = tp.Rotation;
            t.localScale = tp.LocalScale;
        }

        /// <summary>
        /// Sets a transform to local properties.
        /// </summary>
        public static void SetLocalProperties(this Transform t, TransformProperties tp)
        {
            t.localPosition = tp.Position;
            t.localRotation = tp.Rotation;
            t.localScale = tp.Scale;
        }

        /// <summary>
        /// Sets a transform to world properties.
        /// </summary>
        public static void SetWorldProperties(this Transform t, TransformPropertiesCls tp)
        {
            t.position = tp.Position;
            t.rotation = tp.Rotation;
            t.localScale = tp.LocalScale;
        }

        /// <summary>
        /// Sets a transform to world properties.
        /// </summary>
        public static void SetWorldProperties(this Transform t, TransformProperties tp)
        {
            t.position = tp.Position;
            t.rotation = tp.Rotation;
            t.localScale = tp.Scale;
        }

        /// <summary>
        /// Sets local position and rotation for a transform.
        /// </summary>
        public static void SetLocalPositionAndRotation(this Transform t, Vector3 pos, Quaternion rot)
        {
            t.localPosition = pos;
            t.localRotation = rot;
        }

        /// <summary>
        /// Sets local position, rotation, and scale for a transform.
        /// </summary>
        public static void SetLocalPositionRotationAndScale(this Transform t, Vector3 pos, Quaternion rot, Vector3 scale)
        {
            t.localPosition = pos;
            t.localRotation = rot;
            t.localScale = scale;
        }

        /// <summary>
        /// Sets local position, rotation, and scale using nullables for a transform. If a value is null then that property is skipped.
        /// </summary>
        public static void SetLocalPositionRotationAndScale(this Transform t, Vector3? nullablePos, Quaternion? nullableRot, Vector3? nullableScale)
        {
            if (nullablePos.HasValue)
                t.localPosition = nullablePos.Value;
            if (nullableRot.HasValue)
                t.localRotation = nullableRot.Value;
            if (nullableScale.HasValue)
                t.localScale = nullableScale.Value;
        }

        /// <summary>
        /// Sets world position, rotation, and scale using nullables for a transform. If a value is null then that property is skipped.
        /// </summary>
        public static void SetWorldPositionRotationAndScale(this Transform t, Vector3? nullablePos, Quaternion? nullableRot, Vector3? nullableScale)
        {
            if (nullablePos.HasValue)
                t.position = nullablePos.Value;
            if (nullableRot.HasValue)
                t.rotation = nullableRot.Value;
            if (nullableScale.HasValue)
                t.localScale = nullableScale.Value;
        }

        /// <summary>
        /// Oututs properties to use for a transform. When a nullable property has value that value is used, otherwise the transforms current property is used.
        /// </summary>
        public static void OutLocalPropertyValues(this Transform t, Vector3? nullablePos, Quaternion? nullableRot, Vector3? nullableScale, out Vector3 pos, out Quaternion rot, out Vector3 scale)
        {
            pos = nullablePos == null ? t.localPosition : nullablePos.Value;
            rot = nullableRot == null ? t.localRotation : nullableRot.Value;
            scale = nullableScale == null ? t.localScale : nullableScale.Value;
        }

        /// <summary>
        /// Oututs properties to use for a transform. When a nullable property has value that value is used, otherwise the transforms current property is used.
        /// </summary>
        public static void OutWorldPropertyValues(this Transform t, Vector3? nullablePos, Quaternion? nullableRot, Vector3? nullableScale, out Vector3 pos, out Quaternion rot, out Vector3 scale)
        {
            pos = nullablePos == null ? t.position : nullablePos.Value;
            rot = nullableRot == null ? t.rotation : nullableRot.Value;
            scale = nullableScale == null ? t.localScale : nullableScale.Value;
        }
    }
}