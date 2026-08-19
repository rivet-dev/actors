using FishNet.CodeGenerating.Extension;
using MonoFN.Cecil;

namespace FishNet.CodeGenerating.Helping.Extension
{
    internal static class FieldReferenceExtensions
    {
        /// <summary>
        /// Gets a Resolve favoring cached results first.
        /// </summary>
        internal static FieldDefinition CachedResolve(this FieldReference fieldRef, CodegenSession session)
        {
            return session.GetClass<GeneralHelper>().GetFieldReferenceResolve(fieldRef);
        }

        /// <summary>
        /// Makes a FieldReference generic if it has generic parameters.
        /// </summary>
        public static FieldReference TryMakeGenericInstance(this FieldReference fr)
        {
            TypeReference declaringTr = fr.DeclaringType;

            if (declaringTr.HasGenericParameters)
            {
                GenericInstanceType git = declaringTr.MakeGenericInstanceType();
                FieldReference result = new(fr.Name, fr.FieldType, git);
                return result;
            }
            else
            {
                return fr;
            }
        }
    }
}