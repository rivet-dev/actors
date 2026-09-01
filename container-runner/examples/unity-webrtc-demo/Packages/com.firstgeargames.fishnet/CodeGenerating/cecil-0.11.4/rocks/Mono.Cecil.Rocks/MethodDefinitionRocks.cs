//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using System;

namespace MonoFN.Cecil.Rocks
{
#if UNITY_EDITOR
    public
#endif
        static class MethodDefinitionRocks
    {
        public static MethodDefinition GetBaseMethod(this MethodDefinition self)
        {
            if (self == null)
                throw new ArgumentNullException("self");
            if (!self.IsVirtual)
                return self;
            if (self.IsNewSlot)
                return self;

            TypeDefinition base_type = ResolveBaseType(self.DeclaringType);
            while (base_type != null)
            {
                MethodDefinition @base = GetMatchingMethod(base_type, self);
                if (@base != null)
                    return @base;

                base_type = ResolveBaseType(base_type);
            }

            return self;
        }

        public static MethodDefinition GetOriginalBaseMethod(this MethodDefinition self)
        {
            if (self == null)
                throw new ArgumentNullException("self");

            while (true)
            {
                MethodDefinition @base = self.GetBaseMethod();
                if (@base == self)
                    return self;

                self = @base;
            }
        }

        private static TypeDefinition ResolveBaseType(TypeDefinition type)
        {
            if (type == null)
                return null;

            TypeReference base_type = type.BaseType;
            if (base_type == null)
                return null;

            return base_type.Resolve();
        }

        private static MethodDefinition GetMatchingMethod(TypeDefinition type, MethodDefinition method)
        {
            return MetadataResolver.GetMethod(type.Methods, method);
        }
    }
}