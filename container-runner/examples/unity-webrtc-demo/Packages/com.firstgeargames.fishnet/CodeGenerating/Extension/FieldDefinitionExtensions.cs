using MonoFN.Cecil;

namespace FishNet.CodeGenerating.Helping.Extension
{
    internal static class FieldDefinitionExtensions
    {
        /// <summary>
        /// Makes a FieldDefinition generic if it has generic parameters.
        /// </summary>
        public static FieldReference TryMakeGenericInstance(this FieldDefinition fd, CodegenSession session)
        {
            FieldReference fr = session.ImportReference(fd);
            return fr.TryMakeGenericInstance();
        }
    }
}