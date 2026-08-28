<template>
  <div 
    class="absolute top-0 left-0 w-full z-50 flex items-center justify-between px-4 py-2 bg-black/60 backdrop-blur-md transition-all duration-300 pointer-events-auto shadow-lg"
    :class="show ? 'opacity-100 translate-y-0' : 'opacity-0 -translate-y-2 pointer-events-none'"
  >
    <!-- 左侧：歌曲信息 -->
    <div class="text-white text-sm font-medium truncate flex-1 mr-4">
      <template v-if="title && artist">
        {{ title }} — {{ artist }}
      </template>
      <template v-else-if="title">
        {{ title }}
      </template>
      <template v-else>
        等待播放...
      </template>
    </div>
    
    <!-- 右侧：控制按钮 -->
    <div class="flex items-center space-x-3 text-white flex-shrink-0">
      <!-- 上一曲 -->
      <button @click="emit('prev')" class="hover:text-pink-400 transition-colors duration-200 active:scale-90" title="上一曲">
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="19 20 9 12 19 4 19 20"></polygon><line x1="5" y1="19" x2="5" y2="5"></line></svg>
      </button>
      
      <!-- 播放/暂停 (根据状态切换图标) -->
      <button @click="emit('playPause')" class="hover:text-pink-400 transition-colors duration-200 active:scale-90" title="播放/暂停">
        <!-- 播放中 -> 显示暂停图标 -->
        <svg v-if="isPlaying" xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="10"></circle>
          <line x1="10" y1="15" x2="10" y2="9"></line>
          <line x1="14" y1="15" x2="14" y2="9"></line>
        </svg>
        <!-- 暂停中 -> 显示播放图标 -->
        <svg v-else xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="10"></circle>
          <polygon points="10 8 16 12 10 16 10 8"></polygon>
        </svg>
      </button>
      
      <!-- 下一曲 -->
      <button @click="emit('next')" class="hover:text-pink-400 transition-colors duration-200 active:scale-90" title="下一曲">
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="5 4 15 12 5 20 5 4"></polygon><line x1="19" y1="5" x2="19" y2="19"></line></svg>
      </button>

      <div class="w-px h-4 bg-gray-500 mx-1"></div>

      <!-- 设置 -->
      <button @click="emit('settings')" class="hover:text-gray-300 transition-colors duration-200 active:scale-90" title="设置">
        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"></circle><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"></path></svg>
      </button>

      <!-- 关闭 -->
      <button @click="emit('close')" class="hover:text-red-500 transition-colors duration-200 active:scale-90" title="关闭">
        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"></line><line x1="6" y1="6" x2="18" y2="18"></line></svg>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  show: boolean;
  title: string;
  artist: string;
  isPlaying: boolean;
}>();

const emit = defineEmits(['prev', 'playPause', 'next', 'settings', 'close']);
</script>
